use crate::{ChannelItem, ChatLine, FriendItem, NativeApp, ScreenItem, VideoItem};
use base64::Engine;
use futures_util::StreamExt;
use libwebrtc::media_stream_track::MediaStreamTrack;
use libwebrtc::video_stream::native::NativeVideoStream;
use serde_json::json;
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, Model, ModelRc, Timer, TimerMode, VecModel};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Mutex,
};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use zabor_native::attestation::Attestation;
use zabor_native::auth::{
    AuthClient, AuthError, Bootstrap, Captcha, Credentials, LoginOutcome, Registration,
};
use zabor_native::chat::{ChatEngine, ChatError};
use zabor_native::chat_crypto::ChatIdentity;
use zabor_native::chat_store::{ChatMessage, ChatStore};
use zabor_native::chat_transport::{ChatTransport, ChatTransportEvent};
use zabor_native::hub::{HubConnection, HubEvent};
use zabor_native::media::{MediaError, MediaEvent};
use zabor_native::profile::Profile;
use zabor_native::screen::{ScreenCapture, ScreenSource, SourceKind};
use zabor_native::strings::Strings;
use zabor_native::video::{decode_frame, DecodedVideo};
use zabor_native::voice::VoiceSession;
use zeroize::Zeroizing;

enum Command {
    Authenticate {
        credentials: Credentials,
        registration: Option<Registration>,
    },
    Captcha,
    Modal {
        kind: i32,
        value: String,
    },
    Logout,
    Join(String),
    Leave,
    Audio(bool, bool),
    Reconnect,
    Language(String),
    ChatOpen(String),
    ChatSend(String, String),
    ChatDelete(String, String),
    PruneChat,
    Call(String),
    AnswerCall(String, bool),
    ListScreens,
    StartShare(String),
    StopShare,
    CheckShare,
}

enum UiEvent {
    Connected,
    MissingAccount,
    SignedIn(Box<Bootstrap>),
    Captcha(Captcha),
    Error(AuthError),
    Data(Box<Bootstrap>),
    Hub(HubEvent),
    Disconnected,
    SignedOut,
    Avatar(String),
    Voice(String, Vec<zabor_native::auth::User>),
    VoiceEnded,
    Toast(&'static str),
    ChatMessages(String, Vec<ChatMessage>, bool),
    IncomingCall(String, String),
    Calling(String),
    IncomingDismissed,
    Screens(Vec<ScreenSource>),
    Sharing(bool),
    RemoveVideo(String),
}

struct FrameUpdate {
    peer: String,
    generation: u64,
    alive: Arc<AtomicBool>,
    frame: DecodedVideo,
}
type VideoSink = Arc<dyn Fn(FrameUpdate) + Send + Sync>;
struct VideoSubscription {
    task: tokio::task::JoinHandle<()>,
    alive: Arc<AtomicBool>,
}
impl Drop for VideoSubscription {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
        self.task.abort();
    }
}

pub fn run(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut locale = "ru".to_owned();
    let mut server = std::env::var("ZABOR_SERVER_URL")
        .unwrap_or_else(|_| "https://vnkboltik.ru:8080/zabor_v3".into());
    let mut snapshot: Option<PathBuf> = None;
    let mut preview = None;
    let mut reduced_motion = false;
    let mut profile_path: Option<PathBuf> = None;
    let mut interaction_check = false;
    let mut index = 0;
    while index < arguments.len() {
        let option = arguments[index].as_str();
        if option == "--app" {
            index += 1;
            continue;
        }
        if option == "--reduced-motion" {
            reduced_motion = true;
            index += 1;
            continue;
        }
        if option == "--interaction-check" {
            interaction_check = true;
            index += 1;
            continue;
        }
        let value = arguments
            .get(index + 1)
            .ok_or("Missing application argument")?;
        match option {
            "--locale" => locale = value.clone(),
            "--server" => server = value.clone(),
            "--profile" => profile_path = Some(value.into()),
            "--snapshot" => snapshot = Some(value.into()),
            "--preview" => preview = Some(value.clone()),
            "--scale" if ["1", "1.25", "1.5", "2"].contains(&value.as_str()) => {
                std::env::set_var("SLINT_SCALE_FACTOR", value)
            }
            _ => return Err("Unsupported application argument".into()),
        }
        index += 2;
    }
    zabor_native::hub::validate_endpoint(&server)?;
    let russian = Strings::new("ru")?;
    let english = Strings::new("en")?;
    if !["ru", "en"].contains(&locale.as_str()) {
        return Err("Unsupported locale".into());
    }
    if snapshot.as_ref().is_some_and(|path| path.exists()) {
        return Err("Snapshot exists".into());
    }
    let ui = NativeApp::new()?;
    let profile = if snapshot.is_none() && preview.is_none() && !interaction_check {
        let root = match profile_path {
            Some(path) => path,
            None => Profile::default_path()?,
        };
        Some(Arc::new(Profile::open(&root)?))
    } else {
        None
    };
    if !arguments.iter().any(|argument| argument == "--locale") {
        if let Some(profile) = &profile {
            if let Ok(settings) = profile.read_settings() {
                if let Some(language) = settings["language"]
                    .as_str()
                    .filter(|language| ["ru", "en"].contains(language))
                {
                    locale = language.into();
                }
            }
        }
    }
    ui.on_tr(move |key, language| {
        if language == "en" {
            english.text(&key).into()
        } else {
            russian.text(&key).into()
        }
    });
    ui.on_limit_text(|value, maximum| {
        let mut count = 0;
        value
            .chars()
            .take_while(|character| {
                count += character.len_utf16();
                count <= maximum.max(0) as usize
            })
            .collect::<String>()
            .into()
    });
    ui.set_language(locale.into());
    ui.set_reduced_motion(reduced_motion);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let (commands, receiver) = mpsc::channel(16);
    let stop = CancellationToken::new();
    let captcha_id = Rc::new(RefCell::new(String::new()));
    let setup_avatar = Rc::new(RefCell::new(None::<String>));
    let latest = Rc::new(RefCell::new(Bootstrap::default()));
    let video_generation = Arc::new(AtomicU64::new(0));
    let mailbox = Arc::new(Mutex::new(BTreeMap::<String, FrameUpdate>::new()));
    let scheduled = Arc::new(AtomicBool::new(false));
    let video_mailbox = mailbox.clone();
    let video_scheduled = scheduled.clone();
    let video_ui = ui.as_weak();
    let video_sink: VideoSink = Arc::new(move |frame| {
        if let Ok(mut mailbox) = video_mailbox.lock() {
            mailbox.insert(frame.peer.clone(), frame);
        }
        if !video_scheduled.swap(true, Ordering::AcqRel) {
            let weak = video_ui.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    ui.invoke_process_video();
                }
            });
        }
    });
    let weak = ui.as_weak();
    let expected_video_generation = video_generation.clone();
    ui.on_process_video(move || {
        scheduled.store(false, Ordering::Release);
        let frames = mailbox
            .lock()
            .map(|mut frames| std::mem::take(&mut *frames))
            .unwrap_or_default();
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if !ui.get_voice_active() {
            return;
        }
        for (peer, frame) in frames {
            if frame.generation != expected_video_generation.load(Ordering::Acquire)
                || !frame.alive.load(Ordering::Acquire)
            {
                continue;
            }
            let Some(user) = ui.get_voice_users().iter().find(|user| user.id == peer) else {
                continue;
            };
            let pixels = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                &frame.frame.rgba,
                frame.frame.width,
                frame.frame.height,
            );
            let video = VideoItem {
                id: peer.into(),
                title: user.name,
                frame: slint::Image::from_rgba8(pixels),
            };
            let model = ui.get_video_frames();
            if let Some(index) = model.iter().position(|entry| entry.id == video.id) {
                model.set_row_data(index, video);
            } else {
                let mut entries = model.iter().collect::<Vec<_>>();
                if entries.len() < 3 {
                    entries.push(video);
                    ui.set_video_frames(Rc::new(VecModel::from(entries)).into());
                }
            }
        }
    });
    let (updates, updates_rx) = std::sync::mpsc::sync_channel(128);
    let wake_ui = ui.as_weak();
    let overflow_stop = stop.clone();
    let send_update = Arc::new(move |event| {
        let overflow = updates.try_send(event).is_err();
        if overflow {
            overflow_stop.cancel();
        }
        let weak = wake_ui.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                ui.invoke_process_updates();
                if overflow {
                    ui.set_connected(false);
                    ui.set_busy(false);
                    show_error(&ui, AuthError::Network);
                }
            }
        });
    });
    let worker = runtime.spawn(worker(
        server,
        receiver,
        send_update.clone(),
        stop.clone(),
        profile,
        video_sink,
        video_generation,
    ));
    let weak = ui.as_weak();
    let auth_commands = commands.clone();
    let auth_captcha = captcha_id.clone();
    let auth_avatar = setup_avatar.clone();
    ui.on_submit_auth(move || {
        if let Some(ui) = weak.upgrade() {
            if ui.get_busy() {
                return;
            }
            let credentials = Credentials {
                username: ui.get_login().to_string(),
                password: Zeroizing::new(ui.get_password().to_string()),
            };
            if let Err(error) = zabor_native::auth::validate_credentials(&credentials) {
                show_error(&ui, error);
                return;
            }
            let registration = (ui.get_auth_step() == 2).then(|| Registration {
                name: ui.get_display_name().to_string(),
                avatar: auth_avatar.borrow().clone(),
                color: "#C81E70".into(),
                captcha_id: auth_captcha.borrow().clone(),
                answer: ui.get_captcha_answer().to_string(),
            });
            ui.set_error("".into());
            ui.set_busy(true);
            if auth_commands
                .try_send(Command::Authenticate {
                    credentials,
                    registration,
                })
                .is_err()
            {
                ui.set_busy(false);
                show_error(&ui, AuthError::Network);
            }
        }
    });
    let weak = ui.as_weak();
    let captcha_commands = commands.clone();
    ui.on_load_captcha(move || {
        if let Some(ui) = weak.upgrade() {
            if ui.get_captcha_busy() {
                return;
            }
            ui.set_captcha_answer("".into());
            ui.set_captcha_busy(true);
            if captcha_commands.try_send(Command::Captcha).is_err() {
                ui.set_captcha_busy(false);
                show_error(&ui, AuthError::Network);
            }
        }
    });
    let weak = ui.as_weak();
    let modal_commands = commands.clone();
    ui.on_submit_modal(move |kind, value| {
        if let Some(ui) = weak.upgrade() {
            if ui.get_busy() {
                return;
            }
            ui.set_busy(true);
            ui.set_error("".into());
            if modal_commands
                .try_send(Command::Modal {
                    kind,
                    value: value.to_string(),
                })
                .is_err()
            {
                ui.set_busy(false);
                show_error(&ui, AuthError::Network);
            }
        }
    });
    let logout_commands = commands.clone();
    ui.on_logout(move || {
        let _ = logout_commands.try_send(Command::Logout);
    });
    let language_commands = commands.clone();
    ui.on_language_changed(move |language| {
        let _ = language_commands.try_send(Command::Language(language.to_string()));
    });
    let image_runtime = runtime.handle().clone();
    ui.on_choose_avatar(move || {
        let update = send_update.clone();
        image_runtime.spawn(async move {
            let Some(file) = rfd::AsyncFileDialog::new()
                .add_filter("images", &["png", "jpg", "jpeg", "webp", "gif"])
                .pick_file()
                .await
            else {
                return;
            };
            let path = file.path().to_owned();
            let image = tokio::task::spawn_blocking(move || -> Result<String, ()> {
                if std::fs::metadata(&path).map_err(|_| ())?.len() > 16 * 1024 * 1024 {
                    return Err(());
                }
                let mut reader = image::ImageReader::open(path)
                    .map_err(|_| ())?
                    .with_guessed_format()
                    .map_err(|_| ())?;
                let mut limits = image::Limits::default();
                limits.max_image_width = Some(8192);
                limits.max_image_height = Some(8192);
                limits.max_alloc = Some(128 * 1024 * 1024);
                reader.limits(limits);
                let image = reader.decode().map_err(|_| ())?;
                let side = image.width().min(image.height());
                let image = image
                    .crop_imm(
                        (image.width() - side) / 2,
                        (image.height() - side) / 2,
                        side,
                        side,
                    )
                    .resize_exact(256, 256, image::imageops::FilterType::Lanczos3);
                let mut bytes = std::io::Cursor::new(Vec::new());
                image
                    .write_to(&mut bytes, image::ImageFormat::Png)
                    .map_err(|_| ())?;
                Ok(format!(
                    "data:image/png;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
                ))
            })
            .await;
            match image {
                Ok(Ok(image)) => update(UiEvent::Avatar(image)),
                _ => update(UiEvent::Error(AuthError::Network)),
            }
        });
    });
    let weak = ui.as_weak();
    let channel_state = latest.clone();
    let channel_commands = commands.clone();
    ui.on_choose_channel(move |id| {
        if let Some(ui) = weak.upgrade() {
            if !ui.get_busy() {
                if let Some(channel) = channel_state
                    .borrow()
                    .channels
                    .iter()
                    .find(|channel| channel.id == id.as_str())
                {
                    ui.set_selected_name(channel.name.clone().into());
                    ui.set_busy(true);
                    if channel_commands
                        .try_send(Command::Join(id.to_string()))
                        .is_err()
                    {
                        ui.set_busy(false);
                        show_error(&ui, AuthError::Network);
                    }
                }
            }
        }
    });
    let leave_commands = commands.clone();
    ui.on_leave_voice(move || {
        let _ = leave_commands.try_send(Command::Leave);
    });
    let audio_commands = commands.clone();
    let audio_state = latest.clone();
    ui.on_audio_state(move |muted, deafened| {
        if audio_commands
            .try_send(Command::Audio(muted, deafened))
            .is_ok()
        {
            let mut state = audio_state.borrow_mut();
            state.user.is_muted = muted;
            state.user.is_deafened = deafened;
        }
    });
    let weak = ui.as_weak();
    let friend_state = latest.clone();
    let chat_commands = commands.clone();
    ui.on_choose_friend(move |id| {
        if let Some(ui) = weak.upgrade() {
            if let Some(friend) = friend_state
                .borrow()
                .friends
                .iter()
                .find(|friend| friend.id == id.as_str())
            {
                ui.set_selected_id(id);
                ui.set_selected_name(friend.display_name.clone().into());
                let _ = chat_commands.try_send(Command::ChatOpen(friend.id.clone()));
            }
        }
    });
    let send_commands = commands.clone();
    ui.on_send_chat(move |peer, text| {
        let _ = send_commands.try_send(Command::ChatSend(peer.to_string(), text.to_string()));
    });
    let delete_commands = commands.clone();
    ui.on_delete_chat(move |peer, id| {
        let _ = delete_commands.try_send(Command::ChatDelete(peer.to_string(), id.to_string()));
    });
    let call_commands = commands.clone();
    ui.on_call_friend(move |peer| {
        let _ = call_commands.try_send(Command::Call(peer.to_string()));
    });
    let answer_commands = commands.clone();
    ui.on_answer_call(move |peer, accept| {
        let _ = answer_commands.try_send(Command::AnswerCall(peer.to_string(), accept));
    });
    let screen_commands = commands.clone();
    ui.on_list_screens(move || {
        let _ = screen_commands.try_send(Command::ListScreens);
    });
    let screen_commands = commands.clone();
    ui.on_start_share(move |source| {
        let _ = screen_commands.try_send(Command::StartShare(source.to_string()));
    });
    let screen_commands = commands.clone();
    ui.on_stop_share(move || {
        let _ = screen_commands.try_send(Command::StopShare);
    });
    let weak = ui.as_weak();
    let close_stop = stop.clone();
    ui.on_window_action(move |action| {
        if let Some(ui) = weak.upgrade() {
            match action.as_str() {
                "close" => {
                    close_stop.cancel();
                    let _ = ui.hide();
                }
                "minimize" => ui.window().set_minimized(true),
                "maximize" => {
                    let maximized = !ui.window().is_maximized();
                    ui.window().set_maximized(maximized);
                }
                "drag" => {
                    ui.window().with_winit_window(|window| {
                        let _ = window.drag_window();
                    });
                }
                _ => {}
            }
        }
    });
    let close_stop = stop.clone();
    ui.window().on_close_requested(move || {
        close_stop.cancel();
        slint::CloseRequestResponse::HideWindow
    });
    let weak = ui.as_weak();
    ui.on_process_updates(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        for _ in 0..32 {
            let Ok(event) = updates_rx.try_recv() else {
                break;
            };
            match event {
                UiEvent::Connected => ui.set_connected(true),
                UiEvent::MissingAccount => {
                    ui.set_busy(false);
                    ui.set_auth_step(1);
                }
                UiEvent::SignedIn(data) => {
                    apply_bootstrap(&ui, &data);
                    *latest.borrow_mut() = *data;
                    ui.set_authenticated(true);
                    ui.set_busy(false);
                    ui.set_password("".into());
                }
                UiEvent::Captcha(captcha) => {
                    *captcha_id.borrow_mut() = captcha.id;
                    ui.set_captcha_image(decode_image(&captcha.image));
                    ui.set_captcha_busy(false);
                }
                UiEvent::Error(error) => {
                    ui.set_busy(false);
                    ui.set_captcha_busy(false);
                    show_error(&ui, error);
                }
                UiEvent::Data(mut data) => {
                    data.channel_users = latest.borrow().channel_users.clone();
                    data.call_user = latest.borrow().call_user.clone();
                    apply_bootstrap(&ui, &data);
                    *latest.borrow_mut() = *data;
                    ui.set_modal(0);
                    ui.set_busy(false);
                }
                UiEvent::Hub(event) => apply_hub_event(&ui, &mut latest.borrow_mut(), event),
                UiEvent::Disconnected => {
                    ui.set_connected(false);
                    ui.set_busy(false);
                }
                UiEvent::SignedOut => {
                    *latest.borrow_mut() = Bootstrap::default();
                    ui.set_authenticated(false);
                    ui.set_auth_step(0);
                    ui.set_modal(0);
                    ui.set_password("".into());
                    ui.set_login("".into());
                    ui.set_busy(false);
                    ui.set_channels(ModelRc::default());
                    ui.set_friends(ModelRc::default());
                    ui.set_chat_lines(ModelRc::default());
                    ui.set_selected_id("".into());
                    ui.set_user_id("".into());
                    ui.set_chat_input("".into());
                }
                UiEvent::Avatar(image) => {
                    ui.set_setup_avatar(decode_image(&image));
                    *setup_avatar.borrow_mut() = Some(image);
                }
                UiEvent::Voice(channel, users) => {
                    {
                        let mut data = latest.borrow_mut();
                        if channel.is_empty() {
                            data.call_user = users.first().cloned();
                            data.user.current_call_user_id =
                                data.call_user.as_ref().map(|user| user.id.clone());
                            data.user.current_channel_id = None;
                        } else {
                            data.call_user = None;
                            data.user.current_call_user_id = None;
                            data.user.current_channel_id = Some(channel.clone());
                            for members in data.channel_users.values_mut() {
                                members.retain(|member| {
                                    !users.iter().any(|user| user.id == member.id)
                                });
                            }
                            data.channel_users.insert(channel.clone(), users.clone());
                        }
                    }
                    ui.set_voice_active(true);
                    if ui.get_modal() == 4 {
                        ui.set_modal(0);
                    }
                    ui.set_active_channel(channel.into());
                    ui.set_voice_users(friend_rows(&users));
                    apply_bootstrap(&ui, &latest.borrow());
                    ui.set_busy(false);
                }
                UiEvent::VoiceEnded => {
                    {
                        let mut data = latest.borrow_mut();
                        let owner = data.user.id.clone();
                        for users in data.channel_users.values_mut() {
                            users.retain(|user| user.id != owner);
                        }
                        data.user.current_channel_id = None;
                        data.user.current_call_user_id = None;
                        data.call_user = None;
                    }
                    ui.set_voice_active(false);
                    ui.set_video_frames(ModelRc::default());
                    ui.set_focused_video("".into());
                    ui.set_sharing(false);
                    ui.set_active_channel("".into());
                    ui.set_voice_users(ModelRc::default());
                    ui.set_busy(false);
                }
                UiEvent::Toast(key) => {
                    ui.set_busy(false);
                    if let Ok(strings) = Strings::new(ui.get_language().as_str()) {
                        ui.set_error(strings.text(key).into());
                    }
                }
                UiEvent::ChatMessages(peer, messages, clear) => {
                    if ui.get_selected_id() == peer {
                        if clear {
                            ui.set_chat_input("".into());
                        }
                        ui.set_chat_lines(
                            Rc::new(VecModel::from(
                                messages
                                    .into_iter()
                                    .map(|message| ChatLine {
                                        id: message.id.into(),
                                        text: message.text.into(),
                                        mine: message.sender_id == ui.get_user_id().as_str(),
                                        delivery: message.delivery.into(),
                                    })
                                    .collect::<Vec<_>>(),
                            ))
                            .into(),
                        );
                    }
                }
                UiEvent::IncomingCall(id, name) => {
                    ui.set_incoming_call_id(id.into());
                    ui.set_incoming_call_name(name.into());
                    ui.set_modal(4);
                    ui.set_busy(false);
                }
                UiEvent::Calling(peer) => {
                    ui.set_voice_active(true);
                    ui.set_active_channel("".into());
                    {
                        let mut data = latest.borrow_mut();
                        data.call_user = data
                            .friends
                            .iter()
                            .find(|friend| friend.id == peer)
                            .cloned();
                        data.user.current_call_user_id = Some(peer.clone());
                        data.user.current_channel_id = None;
                    }
                    if ui.get_modal() == 4 {
                        ui.set_modal(0);
                    }
                    if let Some(friend) = latest
                        .borrow()
                        .friends
                        .iter()
                        .find(|friend| friend.id == peer)
                    {
                        ui.set_voice_users(friend_rows(std::slice::from_ref(friend)));
                    }
                    ui.set_busy(false);
                }
                UiEvent::IncomingDismissed => {
                    if ui.get_modal() == 4 {
                        ui.set_modal(0);
                    }
                    ui.set_incoming_call_id("".into());
                }
                UiEvent::Screens(sources) => {
                    let strings = Strings::new(ui.get_language().as_str()).ok();
                    let rows = sources
                        .into_iter()
                        .map(|source| ScreenItem {
                            id: format!(
                                "{}:{}",
                                match source.kind {
                                    SourceKind::Screen => "screen",
                                    SourceKind::Window => "window",
                                },
                                source.id
                            )
                            .into(),
                            title: if source.title.is_empty() {
                                strings
                                    .as_ref()
                                    .map(|strings| strings.text("stream.screen"))
                                    .unwrap_or_default()
                                    .into()
                            } else {
                                source.title.into()
                            },
                        })
                        .collect::<Vec<_>>();
                    ui.set_screen_sources(Rc::new(VecModel::from(rows)).into());
                    ui.set_selected_screen("".into());
                    ui.set_modal(5);
                }
                UiEvent::Sharing(active) => {
                    ui.set_sharing(active);
                    ui.set_busy(false);
                    if ui.get_modal() == 5 {
                        ui.set_modal(0);
                    }
                }
                UiEvent::RemoveVideo(peer) => {
                    let rows = ui
                        .get_video_frames()
                        .iter()
                        .filter(|row| row.id != peer)
                        .collect::<Vec<_>>();
                    ui.set_video_frames(Rc::new(VecModel::from(rows)).into());
                }
            }
        }
    });
    if let Some(preview) = preview {
        match preview.as_str() {
            "login" => {}
            "confirm" => ui.set_auth_step(1),
            "setup" => ui.set_auth_step(2),
            "home" => {
                ui.set_authenticated(true);
            }
            _ => return Err("Unsupported app preview".into()),
        }
    }
    let capture = Timer::default();
    let capture_error = Rc::new(RefCell::new(None));
    if interaction_check {
        let weak = ui.as_weak();
        let error = capture_error.clone();
        capture.start(
            TimerMode::SingleShot,
            Duration::from_millis(500),
            move || {
                if let Some(ui) = weak.upgrade() {
                    if let Err(failure) = check_auth_interactions(&ui) {
                        *error.borrow_mut() = Some(failure.to_owned());
                    }
                    let _ = ui.hide();
                }
            },
        );
    } else if let Some(path) = snapshot {
        let weak = ui.as_weak();
        let error = capture_error.clone();
        capture.start(
            TimerMode::SingleShot,
            Duration::from_millis(500),
            move || {
                if let Some(ui) = weak.upgrade() {
                    if let Err(failure) = save_snapshot(&ui, &path) {
                        *error.borrow_mut() = Some(failure.to_string());
                    }
                    let _ = ui.hide();
                }
            },
        );
    }
    ui.run()?;
    stop.cancel();
    worker.abort();
    runtime.shutdown_timeout(Duration::from_secs(2));
    if let Some(error) = capture_error.borrow_mut().take() {
        return Err(error.into());
    }
    Ok(())
}

async fn worker(
    server: String,
    mut commands: mpsc::Receiver<Command>,
    update: Arc<dyn Fn(UiEvent) + Send + Sync>,
    stop: CancellationToken,
    profile: Option<Arc<Profile>>,
    video_sink: VideoSink,
    video_generation: Arc<AtomicU64>,
) {
    let signing = match Attestation::from_environment(zabor_native::release::TARGET_BETA_VERSION) {
        Ok(signing) => signing,
        Err(_) => {
            update(UiEvent::Error(AuthError::ClientRejected));
            return;
        }
    };
    let mut hub: Option<HubConnection> = None;
    let mut events: Option<mpsc::Receiver<HubEvent>> = None;
    let mut current_user: Option<zabor_native::auth::User> = None;
    let mut saved_credentials = profile
        .as_ref()
        .and_then(|profile| profile.load_session().ok().flatten())
        .map(|session| Credentials {
            username: session.username,
            password: session.password,
        });
    let mut retry_at = saved_credentials
        .as_ref()
        .map(|_| tokio::time::Instant::now());
    let mut retry_attempt = 0_u32;
    let mut session_ready = false;
    let mut voice: Option<VoiceSession> = None;
    let mut media_events: Option<mpsc::Receiver<MediaEvent>> = None;
    let mut chat: Option<ChatEngine> = None;
    let mut chat_transport: Option<ChatTransport> = None;
    let mut chat_ice = Vec::<zabor_native::media::IceServerSettings>::new();
    let mut chat_failed = false;
    let mut friends = Vec::<String>::new();
    let mut selected_chat: Option<String> = None;
    let mut prune_at = tokio::time::Instant::now() + Duration::from_secs(3600);
    let mut incoming_call: Option<String> = None;
    let mut call_connected = false;
    let mut video_jobs = BTreeMap::<String, VideoSubscription>::new();
    let mut share_watch = tokio::time::interval(Duration::from_secs(1));
    share_watch.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut screen_sources = Vec::<ScreenSource>::new();
    loop {
        if voice.is_none() && !video_jobs.is_empty() {
            video_generation.fetch_add(1, Ordering::AcqRel);
            video_jobs.clear();
        }
        if chat.is_none() && !chat_failed {
            if let (Some(user), Some(profile)) = (current_user.as_ref(), profile.as_ref()) {
                match initialize_chat(profile, &user.id, &friends) {
                    Ok(engine) => {
                        chat = Some(engine);
                        chat_transport = ChatTransport::new(
                            user.id.clone(),
                            friends.iter().cloned(),
                            chat_ice.clone(),
                            false,
                        )
                        .ok();
                    }
                    Err(_) => {
                        chat_failed = true;
                        update(UiEvent::Toast("chat.storageFailed"));
                    }
                }
            }
        }
        let command = tokio::select! {
            biased;
            _ = stop.cancelled() => break,
            command = commands.recv() => { let Some(command) = command else { break; }; command },
            _ = async { if voice.as_ref().is_some_and(VoiceSession::is_sharing) { share_watch.tick().await; } else { std::future::pending::<()>().await; } } => Command::CheckShare,
            _ = tokio::time::sleep_until(prune_at) => { prune_at = tokio::time::Instant::now() + Duration::from_secs(3600); Command::PruneChat },
            _ = async { match retry_at { Some(deadline) => tokio::time::sleep_until(deadline).await, None => std::future::pending().await } } => { retry_at = None; Command::Reconnect },
            event = async { match events.as_mut() { Some(events) => events.recv().await, None => std::future::pending().await } } => {
                match event {
                    Some(event) => {
                        if let Some(user) = current_user.as_mut() { zabor_native::roster::apply_user_event(user, &event, true); }
                        if matches!(event.target.as_str(), "UserStoppedStreaming" | "StreamDropped" | "UserLeft") {
                            if let Some(peer) = event.arguments.first().and_then(serde_json::Value::as_str) { video_jobs.remove(peer); update(UiEvent::RemoveVideo(peer.into())); }
                        }
                        if matches!(event.target.as_str(), "FriendAdded" | "FriendRequestAccepted") {
                            if let Some(user) = event.arguments.first().and_then(|value| serde_json::from_value::<zabor_native::auth::User>(value.clone()).ok()) { if !user.id.is_empty() && current_user.as_ref().is_none_or(|owner| owner.id != user.id) && !friends.contains(&user.id) { friends.push(user.id); } }
                        }
                        if matches!(event.target.as_str(), "FriendRemoved" | "UserDeleted") {
                            if let Some(id) = event.arguments.first().and_then(serde_json::Value::as_str) { friends.retain(|friend| friend != id); if selected_chat.as_deref() == Some(id) { selected_chat = None; } }
                        }
                        if matches!(event.target.as_str(), "FriendAdded" | "FriendRequestAccepted" | "FriendRemoved" | "UserDeleted") {
                            if let Some(chat) = chat.as_mut() { chat.set_friends(friends.iter().cloned()); }
                            if let Some(transport) = chat_transport.as_mut() { transport.set_friends(&friends); }
                        }
                        if let (Some(transport), Some(hub), Some(peer), Some(payload)) = (chat_transport.as_mut(), hub.as_ref(), event.arguments.first().and_then(serde_json::Value::as_str), event.arguments.get(1).and_then(serde_json::Value::as_str)) {
                            if payload.starts_with("zabor-chat:") { let _ = transport.handle_signal(hub, &event.target, peer, payload).await; }
                        }
                        match event.target.as_str() {
                            "IncomingCall" => {
                                if let Some(call) = event.arguments.first() {
                                    if let Some(id) = call["callerId"].as_str() {
                                        incoming_call = Some(id.into());
                                        update(UiEvent::IncomingCall(id.into(), call["callerName"].as_str().unwrap_or("").into()));
                                    }
                                }
                            },
                            "CallAccepted" | "CallStarted" => {
                                if let (Some(session), Some(hub), Some(value)) = (voice.as_mut(), hub.as_ref(), event.arguments.first()) {
                                    if let Ok(user) = serde_json::from_value::<zabor_native::auth::User>(value.clone()) {
                                        if session.call_peer.as_deref() == Some(user.id.as_str()) {
                                            call_connected = true;
                                            incoming_call = None;
                                            if event.target == "CallAccepted" { let _ = session.connect_call(hub, &user.id).await; }
                                            update(UiEvent::Voice(String::new(), vec![user]));
                                        }
                                    }
                                }
                            },
                            "CallEnded" | "CallDeclined" => {
                                if voice.as_ref().is_some_and(|session| session.call_peer.is_some()) && (event.target != "CallDeclined" || !call_connected) {
                                    voice = None; media_events = None; call_connected = false; incoming_call = None; update(UiEvent::VoiceEnded);
                                }
                                else if incoming_call.take().is_some() { update(UiEvent::IncomingDismissed); }
                            },
                            _ => {},
                        }
                        if event.target == "ReceiveChatRelay" {
                            if let (Some(engine), Some(hub), Some(peer), Some(payload)) = (chat.as_mut(), hub.as_ref(), event.arguments.first().and_then(serde_json::Value::as_str), event.arguments.get(1).and_then(serde_json::Value::as_str)) {
                                match engine.receive(peer, payload, now_ms()) {
                                    Ok(packets) => {
                                        send_chat_packets(hub, chat_transport.as_ref(), packets).await;
                                        if selected_chat.as_deref() == Some(peer) {
                                            if let Ok(packets) = engine.mark_read(peer, now_ms()) { send_chat_packets(hub, chat_transport.as_ref(), packets).await; }
                                            if let Ok(messages) = engine.messages(peer, now_ms()) { update(UiEvent::ChatMessages(peer.into(), messages, false)); }
                                        }
                                    },
                                    Err(ChatError::Identity) => update(UiEvent::Toast("chat.keyChanged")),
                                    Err(_) => {},
                                }
                            }
                        }
                        if let (Some(voice), Some(hub)) = (voice.as_mut(), hub.as_ref()) { let _ = voice.handle(hub, &event.target, &event.arguments).await; }
                        if event.target == "ForceLeaveVoice" || voice.as_ref().is_some_and(|voice| voice.channel.is_none() && voice.call_peer.is_none()) { voice = None; media_events = None; update(UiEvent::VoiceEnded); }
                        update(UiEvent::Hub(event));
                    },
                    None => { events = None; hub = None; session_ready = false; if saved_credentials.is_some() { retry_at = Some(tokio::time::Instant::now() + Duration::from_secs(2)); } update(UiEvent::Disconnected); }
                }
                continue;
            },
            event = async { match media_events.as_mut() { Some(events) => events.recv().await, None => std::future::pending().await } } => {
                match event {
                    Some(MediaEvent::Ice { peer, payload }) => { if let Some(hub) = &hub { let _ = hub.send("SendIceCandidate", vec![json!(peer), json!(payload)]).await; } },
                    Some(MediaEvent::Renegotiate(peer)) => { if let (Some(voice), Some(hub)) = (voice.as_mut(), hub.as_ref()) { let _ = voice.renegotiate(hub, &peer).await; } },
                    Some(MediaEvent::Track { peer, track: MediaStreamTrack::Video(track) }) if video_jobs.len() < 3 || video_jobs.contains_key(&peer) => {
                            let alive = Arc::new(AtomicBool::new(true));
                            let live = alive.clone();
                            let sink = video_sink.clone();
                            let generation = video_generation.load(Ordering::Acquire);
                            let id = peer.clone();
                            let task = tokio::spawn(async move {
                                let mut stream = NativeVideoStream::new(track);
                                while let Some(frame) = stream.next().await {
                                    if !live.load(Ordering::Acquire) { break; }
                                    if let Ok(frame) = decode_frame(frame) { sink(FrameUpdate { peer: id.clone(), generation, alive: live.clone(), frame }); }
                                }
                                stream.close();
                                live.store(false, Ordering::Release);
                            });
                            video_jobs.insert(peer.clone(), VideoSubscription { task, alive });
                            if let Some(hub) = &hub { let _ = hub.send("SendStreamViewState", vec![json!(peer), json!("watching")]).await; }
                    },
                    None => media_events = None,
                    _ => {},
                }
                continue;
            },
            event = async { match chat_transport.as_mut() { Some(transport) => transport.next_event().await, None => std::future::pending().await } } => {
                if let (Some(event), Some(hub), Some(chat)) = (event, hub.as_ref(), chat.as_mut()) {
                    match event {
                        ChatTransportEvent::Ice(peer, payload) => { let _ = hub.send("SendIceCandidate", vec![json!(peer), json!(payload)]).await; },
                        ChatTransportEvent::Ready(peer) => { if let Ok(packet) = chat.open(&peer) { send_chat_packets(hub, chat_transport.as_ref(), vec![packet]).await; } },
                        ChatTransportEvent::Text(peer, payload) => {
                            if let Ok(packets) = chat.receive_direct(&peer, &payload, now_ms()) {
                                send_chat_packets(hub, chat_transport.as_ref(), packets).await;
                                if selected_chat.as_deref() == Some(peer.as_str()) {
                                    if let Ok(packets) = chat.mark_read(&peer, now_ms()) { send_chat_packets(hub, chat_transport.as_ref(), packets).await; }
                                    if let Ok(messages) = chat.messages(&peer, now_ms()) { update(UiEvent::ChatMessages(peer, messages, false)); }
                                }
                            }
                        },
                        ChatTransportEvent::Binary(_, _) => {},
                    }
                }
                continue;
            },
        };
        if matches!(command, Command::Logout) {
            video_generation.fetch_add(1, Ordering::AcqRel);
            video_jobs.clear();
            if let Some(connection) = hub.take() {
                connection.close();
            }
            events = None;
            current_user = None;
            chat = None;
            chat_transport = None;
            chat_failed = false;
            friends.clear();
            selected_chat = None;
            incoming_call = None;
            call_connected = false;
            saved_credentials = None;
            retry_at = None;
            session_ready = false;
            if let Some(profile) = &profile {
                if profile.clear_session().is_err() {
                    update(UiEvent::Toast("validation.sessionSaveError"));
                }
            }
            voice = None;
            media_events = None;
            update(UiEvent::VoiceEnded);
            update(UiEvent::SignedOut);
            continue;
        }
        if matches!(command, Command::Leave) {
            video_generation.fetch_add(1, Ordering::AcqRel);
            video_jobs.clear();
            let was_call = voice
                .as_ref()
                .is_some_and(|session| session.call_peer.is_some());
            let was_sharing = voice.as_ref().is_some_and(VoiceSession::is_sharing);
            voice = None;
            media_events = None;
            update(UiEvent::VoiceEnded);
            if let Some(connection) = hub.as_ref().filter(|connection| connection.is_connected()) {
                if was_sharing {
                    let _ = connection.send("StopStream", Vec::new()).await;
                }
                let _ = connection
                    .send(
                        if was_call { "EndCall" } else { "LeaveChannel" },
                        Vec::new(),
                    )
                    .await;
            }
            continue;
        }
        if let Command::Language(language) = command {
            if let Some(profile) = &profile {
                if ["ru", "en"].contains(&language.as_str()) {
                    if let Ok(mut settings) = profile.read_settings() {
                        settings["language"] = json!(language);
                        if profile.write_settings(&settings).is_err() {
                            update(UiEvent::Toast("validation.sessionSaveError"));
                        }
                    }
                }
            }
            continue;
        }
        if matches!(
            command,
            Command::ChatOpen(_) | Command::ChatSend(_, _) | Command::ChatDelete(_, _)
        ) {
            let Some(engine) = chat.as_mut() else {
                update(UiEvent::Toast("chat.storageFailed"));
                continue;
            };
            let (peer, result, clear) = match command {
                Command::ChatOpen(peer) => {
                    selected_chat = Some(peer.clone());
                    let packets = engine.open(&peer).map(|packet| vec![packet]);
                    (peer, packets, false)
                }
                Command::ChatSend(peer, text) => {
                    let packets = engine.send_text(&peer, &text, now_ms());
                    (peer, packets, true)
                }
                Command::ChatDelete(peer, id) => {
                    let packets = engine.remove(&peer, &id);
                    (peer, packets, false)
                }
                _ => unreachable!(),
            };
            match result {
                Ok(packets) => {
                    if let Some(hub) = hub.as_ref().filter(|hub| hub.is_connected()) {
                        send_chat_packets(hub, chat_transport.as_ref(), packets).await;
                        if let Some(transport) = chat_transport.as_mut() {
                            let _ = transport.connect(hub, &peer).await;
                        }
                    }
                    if let Ok(messages) = engine.messages(&peer, now_ms()) {
                        update(UiEvent::ChatMessages(peer, messages, clear));
                    }
                }
                Err(_) => update(UiEvent::Toast("chat.storageFailed")),
            }
            continue;
        }
        if matches!(command, Command::PruneChat) {
            if let (Some(engine), Some(profile)) = (chat.as_mut(), profile.as_ref()) {
                if engine
                    .prune(now_ms(), &profile.root().join("chat-files"))
                    .is_err()
                {
                    update(UiEvent::Toast("chat.storageFailed"));
                }
                if let Some(peer) = &selected_chat {
                    if let Ok(messages) = engine.messages(peer, now_ms()) {
                        update(UiEvent::ChatMessages(peer.clone(), messages, false));
                    }
                }
            }
            continue;
        }
        if hub.as_ref().is_none_or(|hub| !hub.is_connected()) {
            let token = match signing.as_ref().map(Attestation::token).transpose() {
                Ok(token) => token,
                Err(_) => {
                    update(UiEvent::Error(AuthError::ClientRejected));
                    break;
                }
            };
            let connection = tokio::select! { _ = stop.cancelled() => break, result = HubConnection::connect(&server, token.as_ref().map(|token| token.as_str())) => result };
            match connection {
                Ok((connection, receiver)) => {
                    hub = Some(connection);
                    events = Some(receiver);
                    update(UiEvent::Connected);
                }
                Err(error) => {
                    let error = AuthError::from(error);
                    if saved_credentials.is_some() && error != AuthError::ClientRejected {
                        retry_attempt = retry_attempt.saturating_add(1);
                        retry_at = Some(
                            tokio::time::Instant::now()
                                + Duration::from_secs((2_u64 << retry_attempt.min(4)).min(30)),
                        );
                    }
                    update(UiEvent::Error(error));
                    continue;
                }
            }
        }
        let connection = hub.as_ref().unwrap().clone();
        let auth = AuthClient::new(connection.clone());
        if !session_ready
            && saved_credentials.is_some()
            && !matches!(command, Command::Authenticate { .. } | Command::Captcha)
        {
            let outcome = tokio::select! { _ = stop.cancelled() => break, result = auth.login(saved_credentials.as_ref().unwrap()) => result };
            match outcome {
                Ok(LoginOutcome::SignedIn(data)) => {
                    session_ready = true;
                    retry_attempt = 0;
                    retry_at = None;
                    current_user = Some(data.user.clone());
                    chat_ice = data
                        .ice_config
                        .as_ref()
                        .and_then(|value| serde_json::from_value(value["iceServers"].clone()).ok())
                        .unwrap_or_default();
                    friends = data
                        .friends
                        .iter()
                        .map(|friend| friend.id.clone())
                        .collect();
                    if let Some(chat) = chat.as_mut() {
                        chat.set_friends(friends.iter().cloned());
                    }
                    update(UiEvent::SignedIn(data));
                    if let Some(session) = voice.as_mut() {
                        if let Some(channel) = session.channel.clone() {
                            match session.rejoin(&connection).await {
                                Ok(users) => update(UiEvent::Voice(channel, users)),
                                Err(_) => {
                                    voice = None;
                                    media_events = None;
                                    update(UiEvent::VoiceEnded);
                                    update(UiEvent::Toast("toasts.channelJoinFailed"));
                                }
                            }
                        }
                    }
                }
                Ok(LoginOutcome::NeedsRegistration)
                | Err(AuthError::InvalidPassword | AuthError::ClientRejected) => {
                    saved_credentials = None;
                    if let Some(profile) = &profile {
                        let _ = profile.clear_session();
                    }
                    current_user = None;
                    chat = None;
                    chat_transport = None;
                    chat_failed = false;
                    friends.clear();
                    selected_chat = None;
                    voice = None;
                    media_events = None;
                    update(UiEvent::VoiceEnded);
                    update(UiEvent::SignedOut);
                    continue;
                }
                Err(error) => {
                    let delay = match error {
                        AuthError::Throttled(minutes) => minutes.saturating_mul(60),
                        _ => {
                            retry_attempt = retry_attempt.saturating_add(1);
                            (2_u64 << retry_attempt.min(4)).min(30)
                        }
                    };
                    retry_at =
                        Some(tokio::time::Instant::now() + Duration::from_secs(delay.min(86400)));
                    update(UiEvent::Error(error));
                    continue;
                }
            }
        }
        if matches!(command, Command::Reconnect) {
            continue;
        }
        let operation = async {
            match command {
                Command::Authenticate {
                    credentials,
                    registration,
                } => match registration {
                    Some(registration) => auth
                        .register(&credentials, registration)
                        .await
                        .map(|data| LoginOutcome::SignedIn(Box::new(data))),
                    None => auth.login(&credentials).await,
                }
                .map(|outcome| match outcome {
                    LoginOutcome::NeedsRegistration => UiEvent::MissingAccount,
                    LoginOutcome::SignedIn(data) => {
                        current_user = Some(data.user.clone());
                        chat_ice = data
                            .ice_config
                            .as_ref()
                            .and_then(|value| {
                                serde_json::from_value(value["iceServers"].clone()).ok()
                            })
                            .unwrap_or_default();
                        friends = data
                            .friends
                            .iter()
                            .map(|friend| friend.id.clone())
                            .collect();
                        if let Some(profile) = &profile {
                            if profile.save_session(&data.user.id, &credentials).is_err() {
                                update(UiEvent::Toast("validation.sessionSaveError"));
                            }
                        }
                        saved_credentials = Some(credentials);
                        session_ready = true;
                        retry_attempt = 0;
                        retry_at = None;
                        UiEvent::SignedIn(data)
                    }
                }),
                Command::Captcha => auth.captcha().await.map(UiEvent::Captcha),
                Command::Modal { kind, value } => {
                    let target = if kind == 1 {
                        "CreateChannel"
                    } else {
                        "SendFriendRequest"
                    };
                    if value.trim().is_empty() || value.encode_utf16().count() > 25 {
                        return Err(AuthError::Validation("validation.maxChars"));
                    }
                    connection
                        .invoke(target, vec![json!(value.trim())], Duration::from_secs(20))
                        .await
                        .map_err(AuthError::from)?;
                    let user = current_user.clone().ok_or(AuthError::Network)?;
                    auth.load(user)
                        .await
                        .map(|data| UiEvent::Data(Box::new(data)))
                }
                Command::Logout
                | Command::Reconnect
                | Command::Leave
                | Command::Language(_)
                | Command::ChatOpen(_)
                | Command::ChatSend(_, _)
                | Command::ChatDelete(_, _)
                | Command::PruneChat => unreachable!(),
                Command::Join(channel) => {
                    let Some(user) = current_user.as_ref() else {
                        return Err(AuthError::Network);
                    };
                    if voice.is_none() {
                        let (session, receiver) = VoiceSession::new(&connection, user, false)
                            .await
                            .map_err(|_| AuthError::Network)?;
                        voice = Some(session);
                        media_events = Some(receiver);
                    }
                    match voice.as_mut().unwrap().join(&connection, &channel).await {
                        Ok(users) => {
                            video_jobs.retain(|peer, _| users.iter().any(|user| &user.id == peer));
                            Ok(UiEvent::Voice(channel, users))
                        }
                        Err(error) => {
                            if voice.as_ref().is_some_and(|voice| voice.channel.is_none()) {
                                voice = None;
                                media_events = None;
                            }
                            Ok(UiEvent::Toast(match error {
                                MediaError::Microphone => "toasts.micNotFound",
                                MediaError::ChannelFull => "modals.channelFull.title",
                                _ => "toasts.channelJoinFailed",
                            }))
                        }
                    }
                }
                Command::Audio(muted, deafened) => {
                    if let Some(user) = current_user.as_mut() {
                        user.is_muted = muted;
                        user.is_deafened = deafened;
                    }
                    if let Some(session) = voice.as_mut() {
                        session
                            .mute(&connection, muted, deafened)
                            .await
                            .map_err(|_| AuthError::Network)?;
                    }
                    Ok(UiEvent::Connected)
                }
                Command::Call(peer) => {
                    video_generation.fetch_add(1, Ordering::AcqRel);
                    video_jobs.clear();
                    if !friends.contains(&peer) {
                        return Err(AuthError::Network);
                    }
                    if let Some(mut session) = voice.take() {
                        if session.call_peer.is_some() {
                            let _ = connection.send("EndCall", Vec::new()).await;
                        } else {
                            session.leave(&connection).await;
                        }
                    }
                    media_events = None;
                    call_connected = false;
                    let user = current_user.as_ref().ok_or(AuthError::Network)?;
                    let (mut session, receiver) = VoiceSession::new(&connection, user, false)
                        .await
                        .map_err(|_| AuthError::Network)?;
                    session
                        .prepare_call(&peer)
                        .map_err(|_| AuthError::Network)?;
                    let accepted = connection
                        .invoke("StartCall", vec![json!(peer)], Duration::from_secs(20))
                        .await
                        .map_err(AuthError::from)?;
                    if accepted != true {
                        return Ok(UiEvent::VoiceEnded);
                    }
                    voice = Some(session);
                    media_events = Some(receiver);
                    Ok(UiEvent::Calling(peer))
                }
                Command::ListScreens => {
                    screen_sources = tokio::task::spawn_blocking(ScreenCapture::sources)
                        .await
                        .map_err(|_| AuthError::Network)?;
                    Ok(UiEvent::Screens(screen_sources.clone()))
                }
                Command::StartShare(id) => {
                    let source = screen_sources
                        .iter()
                        .find(|source| {
                            format!(
                                "{}:{}",
                                match source.kind {
                                    SourceKind::Screen => "screen",
                                    SourceKind::Window => "window",
                                },
                                source.id
                            ) == id
                        })
                        .cloned()
                        .ok_or(AuthError::Network)?;
                    if let Some(session) = voice.as_mut() {
                        session
                            .start_share(&connection, source, "high")
                            .await
                            .map_err(|_| AuthError::Network)?;
                        Ok(UiEvent::Sharing(true))
                    } else {
                        Err(AuthError::Network)
                    }
                }
                Command::StopShare => {
                    if let Some(session) = voice.as_mut() {
                        session.stop_share(&connection).await;
                    }
                    Ok(UiEvent::Sharing(false))
                }
                Command::CheckShare => {
                    if let Some(session) = voice.as_mut().filter(|session| session.share_failed()) {
                        session.stop_share(&connection).await;
                        Ok(UiEvent::Sharing(false))
                    } else {
                        Ok(UiEvent::Connected)
                    }
                }
                Command::AnswerCall(peer, accept) => {
                    if incoming_call.as_deref() != Some(peer.as_str()) {
                        return Err(AuthError::Network);
                    }
                    incoming_call = None;
                    if !accept {
                        let _ = connection.send("DeclineCall", vec![json!(peer)]).await;
                        return Ok(UiEvent::IncomingDismissed);
                    }
                    video_generation.fetch_add(1, Ordering::AcqRel);
                    video_jobs.clear();
                    if let Some(mut session) = voice.take() {
                        session.leave(&connection).await;
                    }
                    let user = current_user.as_ref().ok_or(AuthError::Network)?;
                    let (mut session, receiver) = VoiceSession::new(&connection, user, false)
                        .await
                        .map_err(|_| AuthError::Network)?;
                    session
                        .prepare_call(&peer)
                        .map_err(|_| AuthError::Network)?;
                    connection
                        .invoke("AcceptCall", vec![json!(peer)], Duration::from_secs(20))
                        .await
                        .map_err(AuthError::from)?;
                    voice = Some(session);
                    media_events = Some(receiver);
                    call_connected = true;
                    Ok(UiEvent::Calling(peer))
                }
            }
        };
        let result = tokio::select! { _ = stop.cancelled() => break, result = operation => result };
        update(result.unwrap_or_else(UiEvent::Error));
    }
    if let Some(hub) = hub {
        hub.close();
    }
    video_generation.fetch_add(1, Ordering::AcqRel);
    video_jobs.clear();
}

fn show_error(ui: &NativeApp, error: AuthError) {
    if let Ok(strings) = Strings::new(ui.get_language().as_str()) {
        ui.set_error(strings.auth_error(&error).into());
    }
}

fn check_auth_interactions(ui: &NativeApp) -> Result<(), &'static str> {
    use slint::platform::{PointerEventButton, WindowEvent};
    let click = |x, y| {
        let position = slint::LogicalPosition::new(x, y);
        ui.window().dispatch_event(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
        ui.window().dispatch_event(WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        });
    };
    click(550.0, 366.0);
    for character in "abcdefghijklmnopqrstuvwxyz".chars() {
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: character.to_string().into(),
        });
        ui.window().dispatch_event(WindowEvent::KeyReleased {
            text: character.to_string().into(),
        });
    }
    if ui.get_login().as_str() != "abcdefghijklmnopqrstuvwxy" {
        return Err("Native text input or maximum length failed");
    }
    click(688.0, 454.0);
    if !ui.get_show_password() {
        return Err("Password visibility button failed");
    }
    ui.set_login("".into());
    ui.set_password("".into());
    ui.invoke_submit_auth();
    if ui.get_error().is_empty() || ui.get_busy() {
        return Err("Invalid credentials reached the network operation");
    }
    check_roster_bindings(ui)?;
    println!(
        "{}",
        serde_json::json!({"nativeAuthInteractionCheck":"passed","nativeRosterBindingCheck":"passed","nativeTextInput":true,"maxLength":25,"passwordVisibility":true,"validationBeforeNetwork":true})
    );
    Ok(())
}

fn check_roster_bindings(ui: &NativeApp) -> Result<(), &'static str> {
    let peer = zabor_native::auth::User {
        id: "local-fixture-peer".into(),
        username: "fixture".into(),
        display_name: "Fixture".into(),
        is_online: true,
        ..Default::default()
    };
    let mut data = Bootstrap {
        user: zabor_native::auth::User {
            id: "local-fixture-owner".into(),
            ..Default::default()
        },
        friends: vec![peer.clone()],
        ..Default::default()
    };
    ui.set_authenticated(true);
    ui.set_active_channel("local-fixture-channel".into());
    ui.set_voice_active(true);
    apply_hub_event(
        ui,
        &mut data,
        HubEvent {
            target: "ChannelCreated".into(),
            arguments: vec![
                json!({"id":"local-fixture-channel","name":"Fixture","ownerId":"local-fixture-owner"}),
            ],
        },
    );
    apply_hub_event(
        ui,
        &mut data,
        HubEvent {
            target: "UserJoinedChannel".into(),
            arguments: vec![json!(peer), json!("local-fixture-channel")],
        },
    );
    if ui
        .get_channels()
        .row_data(0)
        .is_none_or(|row| row.count != 1)
        || ui.get_voice_users().row_count() != 1
    {
        return Err("Channel occupancy did not update the native models");
    }
    apply_hub_event(
        ui,
        &mut data,
        HubEvent {
            target: "UserSpeaking".into(),
            arguments: vec![json!(peer.id), json!(true)],
        },
    );
    if ui
        .get_voice_users()
        .row_data(0)
        .is_none_or(|row| !row.speaking)
    {
        return Err("Speaking event did not update the native voice card");
    }
    apply_hub_event(
        ui,
        &mut data,
        HubEvent {
            target: "UserStateChanged".into(),
            arguments: vec![json!({"UserId":peer.id,"IsServerMuted":true})],
        },
    );
    if ui
        .get_voice_users()
        .row_data(0)
        .is_none_or(|row| !row.muted)
    {
        return Err("Moderation did not update the native voice card");
    }
    ui.set_active_tab(1);
    ui.set_selected_id(peer.id.clone().into());
    ui.set_chat_input("fixture draft".into());
    apply_hub_event(
        ui,
        &mut data,
        HubEvent {
            target: "FriendRemoved".into(),
            arguments: vec![json!(peer.id)],
        },
    );
    if !ui.get_selected_id().is_empty()
        || !ui.get_chat_input().is_empty()
        || ui.get_friends().row_count() != 0
    {
        return Err("Removed friend left a stale native chat selection");
    }
    apply_hub_event(
        ui,
        &mut data,
        HubEvent {
            target: "UserLeftChannel".into(),
            arguments: vec![json!(peer.id), json!("local-fixture-channel")],
        },
    );
    if ui.get_voice_users().row_count() != 0
        || ui
            .get_channels()
            .row_data(0)
            .is_none_or(|row| row.count != 0)
    {
        return Err("Departed user remained in the native voice model");
    }
    Ok(())
}

fn decode_image(source: &str) -> slint::Image {
    if source.len() > 4 * 1024 * 1024 {
        return slint::Image::default();
    }
    let Some((prefix, encoded)) = source.split_once(',') else {
        return slint::Image::default();
    };
    if ![
        "data:image/png;base64",
        "data:image/jpeg;base64",
        "data:image/gif;base64",
        "data:image/webp;base64",
    ]
    .contains(&prefix)
    {
        return slint::Image::default();
    }
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()
        .and_then(|bytes| slint::Image::load_from_data(&bytes, None).ok())
        .unwrap_or_default()
}

fn color(value: &str) -> slint::Color {
    let value = value.strip_prefix('#').unwrap_or(value);
    u32::from_str_radix(value, 16)
        .ok()
        .filter(|_| value.len() == 6)
        .map(|rgb| slint::Color::from_rgb_u8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8))
        .unwrap_or(slint::Color::from_rgb_u8(200, 30, 112))
}

fn apply_bootstrap(ui: &NativeApp, data: &Bootstrap) {
    ui.set_user_id(data.user.id.clone().into());
    ui.set_muted(data.user.is_muted || data.user.is_server_muted || data.user.is_server_deafened);
    ui.set_deafened(data.user.is_deafened || data.user.is_server_deafened);
    ui.set_user_name(data.user.display_name.clone().into());
    ui.set_user_login(data.user.username.clone().into());
    ui.set_user_color(color(&data.user.avatar_color));
    ui.set_user_avatar(
        data.user
            .avatar_base64
            .as_deref()
            .map(decode_image)
            .unwrap_or_default(),
    );
    ui.set_joke(data.joke.clone().into());
    ui.set_channels(
        Rc::new(VecModel::from(
            data.channels
                .iter()
                .map(|channel| ChannelItem {
                    id: channel.id.clone().into(),
                    name: channel.name.clone().into(),
                    count: data
                        .channel_users
                        .get(&channel.id)
                        .map_or(0, |users| users.len().min(i32::MAX as usize) as i32),
                })
                .collect::<Vec<_>>(),
        ))
        .into(),
    );
    ui.set_friends(friend_rows(&data.friends));
    if ui.get_voice_active() {
        if !ui.get_active_channel().is_empty() {
            if let Some(users) = data.channel_users.get(ui.get_active_channel().as_str()) {
                ui.set_voice_users(friend_rows(users));
            }
        } else if let Some(user) = &data.call_user {
            ui.set_voice_users(friend_rows(std::slice::from_ref(user)));
        }
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

fn initialize_chat(
    profile: &Profile,
    owner: &str,
    friends: &[String],
) -> Result<ChatEngine, ChatError> {
    let identity = match profile.load_identity().map_err(|_| ChatError::Identity)? {
        Some(bytes) => {
            ChatIdentity::import(std::str::from_utf8(&bytes).map_err(|_| ChatError::Identity)?)
                .map_err(|_| ChatError::Identity)?
        }
        None => {
            let identity = ChatIdentity::generate();
            profile
                .save_identity(&identity.export().map_err(|_| ChatError::Identity)?)
                .map_err(|_| ChatError::Identity)?;
            identity
        }
    };
    let store =
        ChatStore::open(&profile.root().join("chat.sqlite3")).map_err(|_| ChatError::Storage)?;
    let mut chat = ChatEngine::new(owner.into(), identity, store);
    chat.set_friends(friends.iter().cloned());
    chat.prune(now_ms(), &profile.root().join("chat-files"))?;
    Ok(chat)
}

async fn send_chat_packets(
    hub: &HubConnection,
    transport: Option<&ChatTransport>,
    packets: Vec<zabor_native::chat::OutboundPacket>,
) {
    for packet in packets {
        if transport.is_some_and(|transport| transport.send_control(&packet)) {
            continue;
        }
        if hub
            .send(
                "SendChatRelay",
                vec![json!(packet.peer), json!(packet.payload)],
            )
            .await
            .is_err()
        {
            break;
        }
    }
}

fn friend_rows(users: &[zabor_native::auth::User]) -> ModelRc<FriendItem> {
    Rc::new(VecModel::from(
        users
            .iter()
            .map(|user| FriendItem {
                id: user.id.clone().into(),
                name: user.display_name.clone().into(),
                username: user.username.clone().into(),
                online: user.is_online,
                speaking: user.is_speaking,
                muted: user.is_muted
                    || user.is_deafened
                    || user.is_server_muted
                    || user.is_server_deafened,
                deafened: user.is_deafened || user.is_server_deafened,
                color: color(&user.avatar_color),
                avatar: user
                    .avatar_base64
                    .as_deref()
                    .map(decode_image)
                    .unwrap_or_default(),
            })
            .collect::<Vec<_>>(),
    ))
    .into()
}

fn apply_hub_event(ui: &NativeApp, data: &mut Bootstrap, event: HubEvent) {
    if zabor_native::roster::apply_event(data, &event) {
        if matches!(
            event.target.as_str(),
            "UserStateChanged" | "UserSpeaking" | "UserStartedStreaming" | "UserStoppedStreaming"
        ) {
            ui.set_muted(
                data.user.is_muted || data.user.is_server_muted || data.user.is_server_deafened,
            );
            ui.set_deafened(data.user.is_deafened || data.user.is_server_deafened);
            for model in [ui.get_friends(), ui.get_voice_users()] {
                for (index, mut row) in model.iter().enumerate() {
                    if let Some(user) = std::iter::once(&data.user)
                        .chain(data.friends.iter())
                        .chain(data.channel_users.values().flatten())
                        .chain(data.call_user.iter())
                        .find(|user| user.id == row.id.as_str())
                    {
                        row.speaking = user.is_speaking;
                        row.muted = user.is_muted
                            || user.is_deafened
                            || user.is_server_muted
                            || user.is_server_deafened;
                        row.deafened = user.is_deafened || user.is_server_deafened;
                        model.set_row_data(index, row);
                    }
                }
            }
        } else {
            apply_bootstrap(ui, data);
        }
        if ui.get_active_tab() == 1
            && !data
                .friends
                .iter()
                .any(|friend| friend.id == ui.get_selected_id().as_str())
        {
            ui.set_selected_id("".into());
            ui.set_selected_name("".into());
            ui.set_chat_lines(ModelRc::default());
            ui.set_chat_input("".into());
        } else if let Some(friend) = data
            .friends
            .iter()
            .find(|friend| friend.id == ui.get_selected_id().as_str())
        {
            ui.set_selected_name(friend.display_name.clone().into());
        }
    }
}

fn save_snapshot(ui: &NativeApp, path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let pixels = ui.window().take_snapshot()?;
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let mut encoder = png::Encoder::new(
        std::io::BufWriter::new(file),
        pixels.width(),
        pixels.height(),
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(pixels.as_bytes())?;
    writer.finish()?;
    Ok(())
}
