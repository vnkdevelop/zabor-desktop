use crate::{
    AudioDeviceItem, AudioSettingsView, ChannelItem, ChatLine, FriendItem, MicTestView, NativeApp,
    NotificationItem, ScreenItem, VideoItem,
};
use base64::Engine;
use libwebrtc::media_stream_track::MediaStreamTrack;
use serde_json::{json, Value};
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, Model, ModelRc, Timer, TimerMode, VecModel};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Mutex,
};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use zabor_native::attestation::Attestation;
use zabor_native::audio_preferences::AudioPreferences;
use zabor_native::audio_runtime::SharedAudio;
use zabor_native::audio_settings::AudioSettings;
use zabor_native::auth::{
    AuthClient, AuthError, Bootstrap, Captcha, Credentials, LoginOutcome, Registration,
};
use zabor_native::call_recovery::{CallRecovery, RecoveryAction, ResumeResult};
use zabor_native::camera::CameraCapture;
use zabor_native::chat::{ChatEngine, ChatError};
use zabor_native::chat_crypto::ChatIdentity;
use zabor_native::chat_store::{ChatMessage, ChatStore};
use zabor_native::chat_transport::{ChatTransport, ChatTransportEvent};
use zabor_native::critical_intents::{CriticalAction, CriticalIntents, IntentScope, IntentTicket};
use zabor_native::file_jobs::{FileEvent, FileJobs};
use zabor_native::hub::{HubConnection, HubEvent};
use zabor_native::media::{MediaError, MediaEvent};
use zabor_native::mic_test::{MicTestIssue, MicTestSnapshot, MicTestStage};
use zabor_native::mic_test_ui::MicTestUi;
use zabor_native::native_audio::NativeAudio;
use zabor_native::picker::{PickerImage, PickerQuery};
use zabor_native::profile::Profile;
use zabor_native::remote_viewer::desktop::{P2pFrameSource, ViewerRequest};
use zabor_native::roster::{respond_notification, NotificationKind};
use zabor_native::screen::{ScreenCapture, ScreenSource, SourceKind};
use zabor_native::strings::Strings;
use zabor_native::video::{
    DecodedVideo, StreamDropGrace, VideoFrameLease, VideoFrameStamp, VideoReceiver,
};
use zabor_native::voice::VoiceSession;
use zeroize::Zeroizing;

mod achievement_io;
mod app_ui;
mod avatar_interactions;
mod avatar_ui;
mod chat_io;
#[cfg(test)]
mod critical_tests;
mod file_io;
mod leave_work;
mod member_actions;
mod member_ui;
mod presence_clock;
mod profile_lookup;
mod rc_desktop;
#[cfg(test)]
mod rc_ui_interactions;
use leave_work::{close_logout_connection, VoiceLeave};
#[cfg(test)]
mod leave_tests;
mod main_session;
mod mic_focus;
mod mic_motion;
mod mic_pressed;
use main_session::MainResult;
#[cfg(test)]
mod main_session_tests;
mod session_io;
use session_io::SessionIo;
#[cfg(test)]
mod session_io_tests;
mod session_work;
use session_work::SessionWork;
mod voice_work;
use voice_work::VoiceWork;
#[cfg(test)]
mod session_tests;
#[cfg(test)]
mod voice_tests;

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
    UpdateProfile(zabor_native::auth::ProfileUpdate, avatar_ui::Query),
    LookupProfile(profile_lookup::Request),
    ProfileAddFriend(profile_lookup::Request),
    Achievements(zabor_native::achievements::AchievementQuery),
    MemberAction(member_actions::Request),
    Logout,
    Join(String),
    Leave,
    Audio,
    CheckAudio,
    Reconnect,
    ResumeCall,
    ChatOpen(String),
    ChatSend(String, String),
    ChatDelete(String, String),
    PruneChat,
    Call(String),
    AnswerCall(String, bool),
    ListScreens(PickerQuery, u64, SourceKind),
    ListCameras(PickerQuery),
    StartShare(String, String, bool, bool),
    ToggleControl(zabor_native::voice::ControlTarget),
    ControlInput(zabor_native::voice::ControlTarget, zabor_input::Packet),
    ReleaseControlRequest(zabor_native::voice::ControlTarget),
    RevokeControlTarget(zabor_native::voice::ControlTarget),
    #[cfg(test)]
    ReleaseControl,
    #[cfg(test)]
    RevokeControl,
    FocusStreamAudio(String),
    StopShare,
    CheckShare,
    MaintainStreams,
    ExpireStreams,
    Notification(NotificationKind, String, bool),
    File {
        owner: String,
        peer: String,
        action: FileAction,
    },
    MaintainFiles,
}

impl Command {
    fn scope(&self) -> IntentScope {
        match self {
            Self::Join(_)
            | Self::Leave
            | Self::Call(_)
            | Self::AnswerCall(_, _)
            | Self::ResumeCall
            | Self::ListScreens(_, _, _)
            | Self::ListCameras(_)
            | Self::StartShare(_, _, _, _)
            | Self::ToggleControl(_)
            | Self::ControlInput(_, _)
            | Self::ReleaseControlRequest(_)
            | Self::RevokeControlTarget(_)
            | Self::FocusStreamAudio(_)
            | Self::StopShare
            | Self::CheckShare
            | Self::MaintainStreams
            | Self::Notification(NotificationKind::Channel, _, true) => IntentScope::Voice,
            #[cfg(test)]
            Self::ReleaseControl | Self::RevokeControl => IntentScope::Voice,
            _ => IntentScope::Session,
        }
    }
}

struct QueuedCommand {
    command: Command,
    ticket: IntentTicket,
}

#[derive(Clone)]
struct CommandQueue {
    sender: mpsc::Sender<QueuedCommand>,
    intents: Arc<CriticalIntents>,
}

impl CommandQueue {
    fn try_send(
        &self,
        command: Command,
    ) -> Result<(), Box<mpsc::error::TrySendError<QueuedCommand>>> {
        let queued = QueuedCommand {
            command,
            ticket: self.intents.ticket(),
        };
        if self.sender.is_closed() {
            return Err(Box::new(mpsc::error::TrySendError::Closed(queued)));
        }
        match &queued.command {
            Command::Leave => self.intents.request(CriticalAction::Leave),
            Command::Logout => self.intents.request(CriticalAction::Logout),
            _ => return self.sender.try_send(queued).map_err(Box::new),
        }
        Ok(())
    }

    async fn send(
        &self,
        command: Command,
    ) -> Result<(), Box<mpsc::error::SendError<QueuedCommand>>> {
        let queued = QueuedCommand {
            command,
            ticket: self.intents.ticket(),
        };
        if self.sender.is_closed() {
            return Err(Box::new(mpsc::error::SendError(queued)));
        }
        match &queued.command {
            Command::Leave => self.intents.request(CriticalAction::Leave),
            Command::Logout => self.intents.request(CriticalAction::Logout),
            _ => return self.sender.send(queued).await.map_err(Box::new),
        }
        Ok(())
    }
}

fn queue_control_command(queue: &CommandQueue, runtime: &tokio::runtime::Handle, command: Command) {
    let ticket = match &command {
        Command::ToggleControl(target)
        | Command::ControlInput(target, _)
        | Command::ReleaseControlRequest(target)
        | Command::RevokeControlTarget(target) => target.input_ticket(),
        _ => None,
    };
    let result = if let Some(ticket) = ticket {
        queue
            .sender
            .try_send(QueuedCommand { command, ticket })
            .map_err(Box::new)
    } else {
        queue.try_send(command)
    };
    if let Err(error) = result {
        if let mpsc::error::TrySendError::Full(queued) = *error {
            if let Command::ControlInput(target, packet) = &queued.command {
                if !matches!(packet, zabor_input::Packet::Move { .. }) {
                    target.emergency_stop();
                }
                return;
            }
            let queue = queue.clone();
            runtime.spawn(async move {
                let _ = queue
                    .intents
                    .run(queued.ticket, IntentScope::Voice, queue.sender.send(queued))
                    .await;
            });
        }
    }
}

enum FileAction {
    Stage(Vec<PathBuf>),
    Save(String, PathBuf),
    Cancel(String),
    Clear(String),
    Reveal(String),
}

enum UiEvent {
    Guarded(IntentTicket, IntentScope, Box<UiEvent>),
    PeerGuarded(zabor_native::media::PeerLifetime, Box<UiEvent>),
    Connected,
    MissingAccount,
    SignedIn(Box<Bootstrap>),
    Captcha(Captcha),
    Error(AuthError),
    Data(Box<Bootstrap>),
    Hub(HubEvent),
    Disconnected,
    SignedOut,
    Avatar(avatar_ui::Event),
    App(app_ui::Event),
    Members(member_ui::Event),
    ProfileSaved(zabor_native::auth::User, avatar_ui::Query),
    ProfileError(avatar_ui::Query, AuthError),
    ProfileLookup(
        profile_lookup::Request,
        Result<Option<zabor_native::auth::User>, AuthError>,
    ),
    ProfileFriendResult(profile_lookup::Request, bool),
    Voice(String, Vec<zabor_native::auth::User>),
    VoiceEnded,
    Toast(&'static str),
    ChatMessages(String, Vec<ChatMessage>, bool),
    IncomingCall(String, String),
    Calling(String),
    IncomingDismissed,
    Screens(PickerQuery, Option<Vec<ScreenSource>>),
    Cameras(PickerQuery, Option<Vec<zabor_camera::Device>>),
    SourcePreview(PickerQuery, String, Option<PickerImage>),
    Sharing(bool),
    RemoteControl(Box<rc_desktop::Update>),
    ControlActivity(zabor_native::voice::ControlTarget),
    RemoveVideo(String),
    NotificationResolved(NotificationKind, String, bool),
    PickedFiles(String, String, Vec<PathBuf>),
    FileProgress(String, String, f32),
    ChooseDestination(String, String),
    AudioDevices(Option<Vec<zabor_native::audio_device::AudioDevice>>),
}

impl UiEvent {
    fn guarded(self, ticket: IntentTicket, scope: IntentScope) -> Self {
        Self::Guarded(ticket, scope, Box::new(self))
    }

    fn for_current(mut self, intents: &CriticalIntents) -> Option<Self> {
        loop {
            match self {
                Self::Guarded(ticket, scope, inner) => {
                    if !intents.is_current(ticket, scope) {
                        return None;
                    }
                    self = *inner;
                }
                Self::PeerGuarded(lifetime, inner) => {
                    if !lifetime.is_alive() {
                        return None;
                    }
                    self = *inner;
                }
                Self::Screens(query, sources) => {
                    return (!query.is_cancelled()).then_some(Self::Screens(query, sources));
                }
                Self::Cameras(query, sources) => {
                    return (!query.is_cancelled()).then_some(Self::Cameras(query, sources));
                }
                Self::SourcePreview(query, source, image) => {
                    return (!query.is_cancelled())
                        .then_some(Self::SourcePreview(query, source, image));
                }
                Self::Avatar(event) => return event.alive().then_some(Self::Avatar(event)),
                Self::ProfileSaved(user, query) => {
                    return query.alive().then_some(Self::ProfileSaved(user, query))
                }
                Self::ProfileError(query, error) => {
                    return query.alive().then_some(Self::ProfileError(query, error))
                }
                Self::ProfileLookup(query, result) => {
                    return query.alive().then_some(Self::ProfileLookup(query, result))
                }
                Self::ProfileFriendResult(query, sent) => {
                    return query
                        .alive()
                        .then_some(Self::ProfileFriendResult(query, sent))
                }
                event => return Some(event),
            }
        }
    }
}

struct FrameUpdate {
    peer: String,
    ticket: IntentTicket,
    generation: u64,
    lease: VideoFrameLease,
    frame: DecodedVideo,
    source_epoch: Option<u64>,
}

#[derive(Clone)]
struct DisplayedFrame {
    ticket: IntentTicket,
    generation: u64,
    stamp: VideoFrameStamp,
    source_epoch: Option<u64>,
    source: Option<Arc<P2pFrameSource>>,
}

impl DisplayedFrame {
    fn from_update(update: &FrameUpdate) -> Self {
        Self {
            ticket: update.ticket,
            generation: update.generation,
            stamp: update.frame.stamp(&update.lease),
            source_epoch: update.source_epoch,
            source: update.frame.p2p_source(),
        }
    }

    fn is_current(
        &self,
        intents: &CriticalIntents,
        generation: &AtomicU64,
        stop: &CancellationToken,
    ) -> bool {
        !stop.is_cancelled()
            && intents.is_current(self.ticket, IntentScope::Voice)
            && self.generation == generation.load(Ordering::Acquire)
            && self.stamp.is_alive()
    }

    fn is_p2p_since(&self, target: &zabor_native::voice::ControlTarget, since: Instant) -> bool {
        target.channel.is_alive() && self.matches_p2p(&target.lifetime, since)
    }

    fn matches_p2p(&self, lifetime: &zabor_native::media::PeerLifetime, since: Instant) -> bool {
        self.source_epoch.is_none()
            && self.source.is_some()
            && self.stamp.is_current_since(lifetime, since)
    }

    fn attach(
        &self,
        target: zabor_native::voice::ControlTarget,
    ) -> zabor_native::voice::ControlTarget {
        target.with_frame(self.stamp.clone(), self.ticket)
    }
}

impl FrameUpdate {
    fn is_current(
        &self,
        intents: &CriticalIntents,
        generation: &AtomicU64,
        stop: &CancellationToken,
    ) -> bool {
        !stop.is_cancelled()
            && intents.is_current(self.ticket, IntentScope::Voice)
            && self.generation == generation.load(Ordering::Acquire)
            && self.lease.is_alive()
    }
}
type VideoSink = Arc<dyn Fn(FrameUpdate) + Send + Sync>;

struct UserProfileResult {
    user: zabor_native::auth::User,
    query: avatar_ui::Query,
}

struct WorkerMedia {
    video_sink: VideoSink,
    video_generation: Arc<AtomicU64>,
    audio_preferences: Arc<AudioPreferences>,
    audio_runtime: Arc<SharedAudio<NativeAudio>>,
    intents: Arc<CriticalIntents>,
    session_io: Arc<SessionIo>,
    achievement_io: Arc<achievement_io::AchievementIo>,
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
    let russian = Rc::new(Strings::new("ru")?);
    let english = Rc::new(Strings::new("en")?);
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
    let names_ru = russian.clone();
    let names_en = english.clone();
    ui.on_tr_name(move |key, name, language| {
        let strings = if language == "en" {
            &names_en
        } else {
            &names_ru
        };
        strings.format(&key, &[("name", &name)]).into()
    });
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
    ui.on_mic_thumb_spring(|progress, pop| {
        zabor_native::motion::mic_thumb_spring(f64::from(progress), pop) as f32
    });
    ui.set_reduced_motion(reduced_motion);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let intents = Arc::new(CriticalIntents::default());
    let (sender, receiver) = mpsc::channel(16);
    let commands = CommandQueue {
        sender,
        intents: intents.clone(),
    };
    let control_targets = Rc::new(RefCell::new(
        Vec::<zabor_native::voice::ControlTarget>::new(),
    ));
    let control_frames = Rc::new(RefCell::new(BTreeMap::<String, DisplayedFrame>::new()));
    let control_authorized = Rc::new(Cell::new(false));
    let control_since = Rc::new(RefCell::new(None::<Instant>));
    let local_settings = match profile
        .as_ref()
        .map(|profile| profile.read_settings())
        .transpose()?
    {
        Some(settings) if settings.is_object() => settings,
        Some(_) => return Err("Profile settings are invalid".into()),
        None => json!({}),
    };
    let audio_preferences = AudioPreferences::new(&local_settings);
    audio_preferences.language(ui.get_language().as_str());
    if let Some(settings) = audio_preferences.settings() {
        apply_audio_settings(&ui, &settings);
        apply_audio_devices(&ui, &[], &settings);
    }
    let audio_controls = audio_preferences.controls.clone();
    let stop = CancellationToken::new();
    let preview_mode = preview.is_some() || snapshot.is_some() || interaction_check;
    let audio_runtime = if preview_mode {
        SharedAudio::new(audio_controls.clone(), stop.child_token(), |_, _, _| {
            Box::pin(async { Err(MediaError::Microphone) })
        })
    } else {
        SharedAudio::native(audio_controls.clone(), stop.child_token())
    };
    let audio_runtime_job = runtime.spawn(audio_runtime.clone().run());
    let mic_test_ui = Rc::new(RefCell::new(MicTestUi::new(audio_runtime.clone())));
    apply_mic_test(&ui, MicTestSnapshot::default());
    let local_ui = ui.as_weak();
    let local_test = mic_test_ui.clone();
    let local_runtime = runtime.handle().clone();
    ui.on_mic_test_context_changed(move || {
        if let Some(ui) = local_ui.upgrade().filter(|_| !preview_mode) {
            synchronize_mic_test(&ui, &mut local_test.borrow_mut(), &local_runtime);
        }
    });
    let local_ui = ui.as_weak();
    let local_test = mic_test_ui.clone();
    let local_runtime = runtime.handle().clone();
    ui.on_mic_test_record(move || {
        if let Some(ui) = local_ui.upgrade().filter(|_| !preview_mode) {
            let mut test = local_test.borrow_mut();
            synchronize_mic_test(&ui, &mut test, &local_runtime);
            test.record(std::time::Instant::now(), &local_runtime);
            apply_mic_test(&ui, test.snapshot(std::time::Instant::now()));
        }
    });
    let local_ui = ui.as_weak();
    let local_test = mic_test_ui.clone();
    let local_runtime = runtime.handle().clone();
    ui.on_mic_test_toggle(move || {
        if let Some(ui) = local_ui.upgrade().filter(|_| !preview_mode) {
            let mut test = local_test.borrow_mut();
            synchronize_mic_test(&ui, &mut test, &local_runtime);
            test.toggle_playback();
            apply_mic_test(&ui, test.snapshot(std::time::Instant::now()));
        }
    });
    let local_ui = ui.as_weak();
    let local_test = mic_test_ui.clone();
    let local_runtime = runtime.handle().clone();
    ui.on_mic_test_seek(move |seconds| {
        if let Some(ui) = local_ui.upgrade().filter(|_| !preview_mode) {
            let mut test = local_test.borrow_mut();
            synchronize_mic_test(&ui, &mut test, &local_runtime);
            test.seek(f64::from(seconds));
            apply_mic_test(&ui, test.snapshot(std::time::Instant::now()));
        }
    });
    let captcha_id = Rc::new(RefCell::new(String::new()));
    let setup_avatar = Rc::new(RefCell::new(None::<String>));
    let avatars = Rc::new(RefCell::new(avatar_ui::AvatarUi::new()));
    let latest = Rc::new(RefCell::new(Bootstrap::default()));
    let app_state = Rc::new(RefCell::new(app_ui::AppUi::default()));
    let app_timer = app_ui::bind(
        &ui,
        app_state.clone(),
        &commands,
        intents.clone(),
        latest.clone(),
        avatars.clone(),
    );
    let member_state = Rc::new(RefCell::new(member_ui::MemberUi::default()));
    let member_timer = member_ui::bind(
        &ui,
        member_state.clone(),
        app_state.clone(),
        commands.clone(),
        intents.clone(),
        latest.clone(),
    );
    let pending_files = Rc::new(RefCell::new(None::<(String, String, Vec<PathBuf>)>));
    let video_generation = Arc::new(AtomicU64::new(0));
    let rc_desktop = Rc::new(RefCell::new(rc_desktop::Desktop::new(
        audio_runtime.clone(),
        runtime.handle().clone(),
        intents.clone(),
        video_generation.clone(),
        commands.clone(),
        preview_mode,
    )));
    let rc_timer = Timer::default();
    let rc_ui = ui.as_weak();
    let maintained_rc = rc_desktop.clone();
    rc_timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
        if let Some(ui) = rc_ui.upgrade() {
            maintained_rc.borrow_mut().maintain(&ui);
        }
    });
    let mailbox = Arc::new(Mutex::new(BTreeMap::<String, FrameUpdate>::new()));
    let scheduled = Arc::new(AtomicBool::new(false));
    let video_mailbox = mailbox.clone();
    let video_scheduled = scheduled.clone();
    let video_ui = ui.as_weak();
    let video_intents = intents.clone();
    let video_stop = stop.clone();
    let sink_generation = video_generation.clone();
    let video_sink: VideoSink = Arc::new(move |frame| {
        if !frame.is_current(&video_intents, &sink_generation, &video_stop) {
            return;
        }
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
    let frame_intents = intents.clone();
    let frame_stop = stop.clone();
    let displayed_frames = control_frames.clone();
    let displayed_targets = control_targets.clone();
    let displayed_authorized = control_authorized.clone();
    let displayed_since = control_since.clone();
    let displayed_rc = rc_desktop.clone();
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
            if !frame.is_current(&frame_intents, &expected_video_generation, &frame_stop) {
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
            let displayed = DisplayedFrame::from_update(&frame);
            let video = VideoItem {
                id: peer.into(),
                title: user.name,
                frame: slint::Image::from_rgba8(pixels),
            };
            let id = video.id.to_string();
            let mut admitted = false;
            let model = ui.get_video_frames();
            if let Some(index) = model.iter().position(|entry| entry.id == video.id) {
                model.set_row_data(index, video);
                admitted = true;
            } else {
                let mut entries = model.iter().collect::<Vec<_>>();
                if entries.len() < 3 {
                    entries.push(video);
                    ui.set_video_frames(Rc::new(VecModel::from(entries)).into());
                    admitted = true;
                }
            }
            if admitted {
                displayed_rc.borrow_mut().decoded(&ui, &id, &displayed);
                displayed_frames.borrow_mut().insert(id, displayed);
            }
        }
        let ready = displayed_authorized.get()
            && displayed_since.borrow().is_some_and(|since| {
                let peer = ui.get_control_viewer();
                displayed_targets
                    .borrow()
                    .iter()
                    .find(|target| target.peer == peer.as_str())
                    .is_some_and(|target| {
                        displayed_frames
                            .borrow()
                            .get(peer.as_str())
                            .is_some_and(|frame| {
                                frame.is_current(
                                    &frame_intents,
                                    &expected_video_generation,
                                    &frame_stop,
                                ) && frame.is_p2p_since(target, since)
                            })
                    })
            });
        ui.set_control_ready(ready && displayed_rc.borrow().ready());
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
    let session_io = SessionIo::new(profile.clone(), intents.clone(), stop.clone());
    let achievement_io =
        achievement_io::AchievementIo::new(profile.clone(), intents.clone(), stop.clone());
    let worker = runtime.spawn(worker(
        server,
        receiver,
        send_update.clone(),
        stop.clone(),
        profile.clone(),
        WorkerMedia {
            video_sink,
            video_generation: video_generation.clone(),
            audio_preferences: audio_preferences.clone(),
            audio_runtime: audio_runtime.clone(),
            intents: intents.clone(),
            session_io: session_io.clone(),
            achievement_io: achievement_io.clone(),
        },
    ));
    let local_errors = send_update.clone();
    let local_settings_job = runtime.spawn(audio_preferences.clone().run_local(
        profile.clone(),
        stop.clone(),
        Arc::new(move || local_errors(UiEvent::Toast("validation.sessionSaveError"))),
    ));
    let remote_errors = send_update.clone();
    let remote_settings_job = runtime.spawn(audio_preferences.clone().run_remote(
        stop.clone(),
        Arc::new(move || remote_errors(UiEvent::Toast("validation.connectError"))),
    ));
    let weak = ui.as_weak();
    let auth_commands = commands.clone();
    let auth_captcha = captcha_id.clone();
    let auth_avatar = setup_avatar.clone();
    ui.on_submit_auth(move || {
        if let Some(ui) = weak.upgrade() {
            if ui.get_busy() || ui.get_crop_active() || ui.get_avatar_loading() {
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
                color: format!(
                    "#{:02x}{:02x}{:02x}",
                    ui.get_avatar_color().red(),
                    ui.get_avatar_color().green(),
                    ui.get_avatar_color().blue()
                ),
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
    let logout_test = mic_test_ui.clone();
    let logout_audio = audio_runtime.clone();
    let logout_ui = ui.as_weak();
    let logout_rc = rc_desktop.clone();
    ui.on_logout(move || {
        logout_rc.borrow_mut().close();
        let _ = logout_commands.try_send(Command::Logout);
        logout_test.borrow_mut().close();
        logout_audio.reset();
        if let Some(ui) = logout_ui.upgrade() {
            ui.set_modal(0);
        }
    });
    let language_preferences = audio_preferences.clone();
    let language_errors = send_update.clone();
    let language_ui = ui.as_weak();
    ui.on_language_changed(move |language| {
        if !language_preferences.language(language.as_str()) {
            language_errors(UiEvent::Toast("validation.sessionSaveError"));
        }
        if let Some(ui) = language_ui.upgrade() {
            refresh_audio_names(&ui);
        }
    });
    let edit_preferences = audio_preferences.clone();
    let weak = ui.as_weak();
    ui.on_audio_setting_changed(move |key, value| {
        let Some(ui) = weak.upgrade().filter(|ui| ui.get_authenticated()) else {
            return;
        };
        let value = if matches!(
            key.as_str(),
            "selectedInput" | "selectedOutput" | "micThresholdMode" | "smartNoiseModel"
        ) {
            Some(json!(value.as_str()))
        } else {
            serde_json::from_str(value.as_str()).ok()
        };
        if value.is_some_and(|value| {
            edit_preferences.edit(ui.get_user_id().as_str(), key.as_str(), value)
        }) {
            if let Some(settings) = edit_preferences.settings() {
                apply_audio_settings(&ui, &settings);
            }
        } else {
            show_error(&ui, AuthError::Validation("validation.sessionSaveError"));
        }
    });
    let volume_preferences = audio_preferences.clone();
    let weak = ui.as_weak();
    ui.on_show_user_volume(move |peer, name| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if peer == ui.get_user_id() || !ui.get_voice_users().iter().any(|user| user.id == peer) {
            return;
        }
        if let Some(settings) = volume_preferences.settings() {
            ui.set_peer_volume(
                settings
                    .user_volumes
                    .get(peer.as_str())
                    .copied()
                    .unwrap_or(100.0),
            );
            ui.set_volume_peer(peer);
            ui.set_volume_name(name);
            ui.set_modal(8);
        }
    });
    let volume_preferences = audio_preferences.clone();
    let weak = ui.as_weak();
    ui.on_peer_volume_changed(move |volume| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let peer = ui.get_volume_peer();
        if ui.get_authenticated()
            && peer != ui.get_user_id()
            && ui.get_voice_users().iter().any(|user| user.id == peer)
            && volume_preferences.peer_volume(ui.get_user_id().as_str(), peer.as_str(), volume)
        {
            ui.set_peer_volume(volume.clamp(0.0, 200.0));
        }
    });
    let enumeration = Arc::new(AtomicBool::new(false));
    let device_runtime = runtime.handle().clone();
    let device_updates = send_update.clone();
    ui.on_refresh_audio_devices(move || {
        if preview_mode || enumeration.swap(true, Ordering::AcqRel) {
            return;
        }
        let enumeration = enumeration.clone();
        let updates = device_updates.clone();
        device_runtime.spawn(async move {
            let devices = tokio::task::spawn_blocking(zabor_native::audio_device::devices).await;
            enumeration.store(false, Ordering::Release);
            updates(UiEvent::AudioDevices(devices.ok().and_then(Result::ok)));
        });
    });
    let device_timer = Timer::default();
    let weak = ui.as_weak();
    device_timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
        if let Some(ui) = weak
            .upgrade()
            .filter(|ui| ui.get_modal() == 3 && ui.get_settings_tab() == 1)
        {
            ui.invoke_refresh_audio_devices();
        }
    });
    let meter_timer = Timer::default();
    let weak = ui.as_weak();
    let meter_test = mic_test_ui.clone();
    let meter_runtime = runtime.handle().clone();
    let mut motion_started = std::time::Instant::now();
    let mut motion_stage = slint::SharedString::default();
    meter_timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_mic_test_text_scale(ui.window().scale_factor());
            if !preview_mode {
                synchronize_mic_test(&ui, &mut meter_test.borrow_mut(), &meter_runtime);
            }
            ui.set_mic_test_revealed(ui.get_mic_test_active());
            if ui.get_mic_test_active() && !ui.get_reduced_motion() {
                let stage = ui.get_mic_test().stage;
                if stage != motion_stage {
                    motion_stage = stage;
                    motion_started = std::time::Instant::now();
                }
                let seconds = motion_started.elapsed().as_secs_f32();
                let phase = 1.0 - (seconds / 0.9 % 2.0 - 1.0).abs();
                ui.set_mic_test_pulse(if phase < 0.5 {
                    2.0 * phase * phase
                } else {
                    1.0 - (-2.0 * phase + 2.0).powi(2) / 2.0
                });
                ui.set_mic_test_dot_pulse(
                    (1.0 - (seconds * std::f32::consts::PI / 0.76).cos()) / 2.0,
                );
            } else {
                ui.set_mic_test_pulse(0.0);
                ui.set_mic_test_dot_pulse(0.0);
            }
        }
    });
    let avatar_timer = avatar_ui::bind(
        &ui,
        avatars.clone(),
        latest.clone(),
        setup_avatar.clone(),
        avatar_ui::Bindings {
            commands: commands.clone(),
            runtime: runtime.handle().clone(),
            update: send_update.clone(),
            intents: intents.clone(),
            stop: stop.clone(),
        },
    );
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
                    if ui.get_voice_active() {
                        if ui.get_active_channel() != id {
                            ui.set_pending_channel(id);
                            ui.set_pending_channel_name(channel.name.clone().into());
                            ui.set_modal(6);
                            ui.set_error("".into());
                        }
                        return;
                    }
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
    let switch_commands = commands.clone();
    let weak = ui.as_weak();
    ui.on_confirm_channel_switch(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_busy() || ui.get_pending_channel().is_empty() {
            return;
        }
        if switch_commands
            .try_send(Command::Join(ui.get_pending_channel().to_string()))
            .is_ok()
        {
            ui.set_busy(true);
            ui.set_active_tab(0);
            ui.set_selected_name(ui.get_pending_channel_name());
        } else {
            show_error(&ui, AuthError::Network);
        }
    });
    let leave_demand = audio_runtime.demand.clone();
    let leave_updates = send_update.clone();
    let leave_ui = ui.as_weak();
    let leave_rc = rc_desktop.clone();
    ui.on_leave_voice(move || {
        leave_rc.borrow_mut().close();
        let _ = leave_commands.try_send(Command::Leave);
        leave_demand.reset();
        if let Some(ui) = leave_ui.upgrade() {
            if ui.get_voice_active() {
                ui.set_voice_active(false);
                leave_updates(UiEvent::VoiceEnded);
            }
        }
    });
    let audio_commands = commands.clone();
    let audio_state = latest.clone();
    let immediate_audio = audio_controls.clone();
    ui.on_audio_state(move |muted, deafened| {
        immediate_audio.set_user_state(muted, deafened);
        let mut state = audio_state.borrow_mut();
        state.user.is_muted = muted;
        state.user.is_deafened = deafened;
        let _ = audio_commands.try_send(Command::Audio);
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
    let file_runtime = runtime.handle().clone();
    let file_update = send_update.clone();
    let weak = ui.as_weak();
    ui.on_choose_files(move |peer| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let owner = ui.get_user_id().to_string();
        let update = file_update.clone();
        let peer = peer.to_string();
        file_runtime.spawn(async move {
            if let Some(files) = rfd::AsyncFileDialog::new().pick_files().await {
                let paths = files
                    .into_iter()
                    .map(|file| file.path().to_owned())
                    .collect();
                update(UiEvent::PickedFiles(owner, peer, paths));
            }
        });
    });
    let file_commands = commands.clone();
    let picked = pending_files.clone();
    let weak = ui.as_weak();
    ui.on_confirm_files(move |send| {
        if let Some((owner, peer, paths)) = picked.borrow_mut().take() {
            if send {
                let _ = file_commands.try_send(Command::File {
                    owner,
                    peer,
                    action: FileAction::Stage(paths),
                });
            }
        }
        if let Some(ui) = weak.upgrade() {
            ui.set_modal(0);
            ui.set_attachment_names("".into());
        }
    });
    let file_commands = commands.clone();
    let file_runtime = runtime.handle().clone();
    let weak = ui.as_weak();
    ui.on_file_action(move |peer, id, action| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let owner = ui.get_user_id().to_string();
        let peer = peer.to_string();
        let id = id.to_string();
        if action == "reveal" {
            let _ = file_commands.try_send(Command::File {
                owner,
                peer,
                action: FileAction::Reveal(id),
            });
        } else if action == "cancel" || action == "clear" {
            let action = if action == "cancel" {
                FileAction::Cancel(id)
            } else {
                FileAction::Clear(id)
            };
            let _ = file_commands.try_send(Command::File {
                owner,
                peer,
                action,
            });
        } else if action == "save" {
            let Some(row) = ui.get_chat_lines().iter().find(|row| row.id == id) else {
                return;
            };
            let name = row.text.replace(['/', '\\'], "_");
            let commands = file_commands.clone();
            file_runtime.spawn(async move {
                if let Some(file) = rfd::AsyncFileDialog::new()
                    .set_file_name(&name)
                    .save_file()
                    .await
                {
                    let _ = commands
                        .send(Command::File {
                            owner,
                            peer,
                            action: FileAction::Save(id, file.path().into()),
                        })
                        .await;
                }
            });
        }
    });
    let delete_commands = commands.clone();
    let drop_update = send_update.clone();
    let weak = ui.as_weak();
    let window_rc = rc_desktop.clone();
    ui.window().on_winit_window_event(move |_, event| {
        use slint::winit_030::{winit::event::WindowEvent, EventResult};
        let Some(ui) = weak.upgrade() else {
            return EventResult::Propagate;
        };
        match event {
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                window_rc.borrow_mut().rescale(*scale_factor);
                if std::env::var("SLINT_SCALE_FACTOR").is_err() {
                    ui.set_mic_test_text_scale(*scale_factor as f32);
                }
            }
            WindowEvent::Resized(_) => window_rc
                .borrow_mut()
                .rescale(f64::from(ui.window().scale_factor())),
            WindowEvent::Focused(focused) => {
                window_rc.borrow_mut().focus(*focused);
                if !focused {
                    ui.invoke_release_control();
                }
            }
            WindowEvent::Destroyed => window_rc.borrow_mut().close(),
            WindowEvent::HoveredFile(_) => ui.set_file_drag(
                ui.get_authenticated()
                    && ui.get_active_tab() == 1
                    && !ui.get_selected_id().is_empty()
                    && ui.get_modal() == 0,
            ),
            WindowEvent::HoveredFileCancelled => ui.set_file_drag(false),
            WindowEvent::DroppedFile(path) => {
                ui.set_file_drag(false);
                if ui.get_authenticated()
                    && ui.get_active_tab() == 1
                    && !ui.get_selected_id().is_empty()
                    && [0, 7].contains(&ui.get_modal())
                {
                    drop_update(UiEvent::PickedFiles(
                        ui.get_user_id().to_string(),
                        ui.get_selected_id().to_string(),
                        vec![path.clone()],
                    ));
                }
            }
            _ => {}
        }
        EventResult::Propagate
    });
    ui.on_delete_chat(move |peer, id| {
        let _ = delete_commands.try_send(Command::ChatDelete(peer.to_string(), id.to_string()));
    });
    let call_commands = commands.clone();
    let weak = ui.as_weak();
    let call_state = latest.clone();
    let call_feedback = app_state.clone();
    ui.on_call_friend(move |peer| {
        let Some(ui) = weak
            .upgrade()
            .filter(|ui| ui.get_authenticated() && ui.get_connected() && !ui.get_busy())
        else {
            return;
        };
        let data = call_state.borrow();
        let Some(friend) = data
            .friends
            .iter()
            .find(|friend| friend.id == peer.as_str())
        else {
            return;
        };
        let online = if ui.get_modal() == 9 && ui.get_profile_id() == peer {
            ui.get_profile_online()
        } else {
            friend.is_online
        };
        if !online {
            call_feedback.borrow_mut().feedback(
                &ui,
                "profile.userOffline",
                std::time::Instant::now(),
            );
            return;
        }
        if call_commands
            .try_send(Command::Call(peer.to_string()))
            .is_ok()
        {
            ui.set_busy(true);
        }
    });
    let answer_commands = commands.clone();
    ui.on_answer_call(move |peer, accept| {
        let _ = answer_commands.try_send(Command::AnswerCall(peer.to_string(), accept));
    });
    let notification_commands = commands.clone();
    let notification_state = latest.clone();
    let weak = ui.as_weak();
    ui.on_respond_notification(move |kind, id, accept| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_busy() || !ui.get_connected() {
            return;
        }
        let data = notification_state.borrow();
        let kind = match kind {
            0 if data.requests.iter().any(|user| user.id == id.as_str()) => {
                NotificationKind::Friend
            }
            1 if data
                .invites
                .iter()
                .any(|invite| invite.channel_id == id.as_str()) =>
            {
                NotificationKind::Channel
            }
            _ => return,
        };
        ui.set_error("".into());
        if notification_commands
            .try_send(Command::Notification(kind, id.to_string(), accept))
            .is_ok()
        {
            ui.set_busy(true);
        } else {
            show_error(&ui, AuthError::Network);
        }
    });
    let picker_query = Rc::new(RefCell::new(None::<PickerQuery>));
    let cancelled_query = picker_query.clone();
    let weak = ui.as_weak();
    ui.on_cancel_picker_query(move || {
        if let Some(query) = cancelled_query.borrow_mut().take() {
            query.cancel();
        }
        if let Some(ui) = weak
            .upgrade()
            .filter(|ui| !ui.get_voice_active() || ui.get_modal() != 5)
        {
            clear_picker_rows(&ui);
        }
    });
    let weak = ui.as_weak();
    ui.on_list_screens(move || {
        let Some(ui) = weak
            .upgrade()
            .filter(|ui| ui.get_voice_active() && !ui.get_busy())
        else {
            return;
        };
        ui.invoke_cancel_picker_query();
        ui.set_screen_sources(ModelRc::default());
        ui.set_window_sources(ModelRc::default());
        ui.set_camera_sources(ModelRc::default());
        ui.set_selected_screen("".into());
        ui.set_screen_source_kind(0);
        ui.set_screen_quality("low".into());
        ui.set_screen_include_audio(true);
        ui.set_screen_allow_control(false);
        ui.set_screen_sources_loading(true);
        ui.set_error("".into());
        ui.set_modal(5);
        ui.invoke_refresh_screens();
    });
    let weak = ui.as_weak();
    let screen_commands = commands.clone();
    let screen_query = picker_query.clone();
    ui.on_refresh_screens(move || {
        let Some(ui) = weak.upgrade().filter(|ui| {
            ui.get_voice_active()
                && !ui.get_busy()
                && ui.get_modal() == 5
                && ui.get_screen_source_kind() != 2
        }) else {
            return;
        };
        let Some(own_window) = native_window_handle(&ui) else {
            ui.set_screen_sources_loading(false);
            show_error(&ui, AuthError::Network);
            return;
        };
        ui.invoke_cancel_picker_query();
        let query = PickerQuery::new(
            screen_commands
                .intents
                .voice_token(screen_commands.intents.ticket()),
        );
        *screen_query.borrow_mut() = Some(query.clone());
        let kind = if ui.get_screen_source_kind() == 0 {
            ui.set_screen_sources_loading(ui.get_screen_sources().row_count() == 0);
            SourceKind::Screen
        } else {
            ui.set_screen_sources_loading(ui.get_window_sources().row_count() == 0);
            SourceKind::Window
        };
        if screen_commands
            .try_send(Command::ListScreens(query, own_window, kind))
            .is_err()
        {
            ui.set_screen_sources_loading(false);
            ui.invoke_cancel_picker_query();
            show_error(&ui, AuthError::Network);
        }
    });
    let weak = ui.as_weak();
    let camera_query = picker_query.clone();
    let camera_commands = commands.clone();
    ui.on_list_cameras(move || {
        let Some(ui) = weak.upgrade().filter(|ui| {
            ui.get_voice_active()
                && !ui.get_busy()
                && ui.get_modal() == 5
                && ui.get_screen_source_kind() == 2
        }) else {
            return;
        };
        ui.invoke_cancel_picker_query();
        let query = PickerQuery::new(
            camera_commands
                .intents
                .voice_token(camera_commands.intents.ticket()),
        );
        *camera_query.borrow_mut() = Some(query.clone());
        ui.set_camera_sources_loading(ui.get_camera_sources().row_count() == 0);
        ui.set_error("".into());
        if camera_commands
            .try_send(Command::ListCameras(query))
            .is_err()
        {
            ui.set_camera_sources_loading(false);
            ui.invoke_cancel_picker_query();
            show_error(&ui, AuthError::Network);
        }
    });
    let picker_refresh = Timer::default();
    let weak = ui.as_weak();
    let refresh_query = picker_query.clone();
    if !preview_mode {
        picker_refresh.start(TimerMode::Repeated, Duration::from_secs(5), move || {
            let Some(ui) = weak
                .upgrade()
                .filter(|ui| ui.get_voice_active() && !ui.get_busy() && ui.get_modal() == 5)
            else {
                return;
            };
            if refresh_query
                .borrow()
                .as_ref()
                .is_some_and(PickerQuery::is_running)
            {
                return;
            }
            if ui.get_screen_source_kind() == 2 {
                ui.invoke_list_cameras();
            } else {
                ui.invoke_refresh_screens();
            }
        });
    }
    let weak = ui.as_weak();
    let screen_commands = commands.clone();
    ui.on_start_share(move |source, quality, include_audio, allow_control| {
        let Some(ui) = weak
            .upgrade()
            .filter(|ui| ui.get_voice_active() && !ui.get_busy())
        else {
            return;
        };
        if source.is_empty()
            || if source.starts_with("camera:") {
                quality != "camera"
            } else {
                !matches!(quality.as_str(), "low" | "high")
            }
        {
            return;
        }
        ui.set_error("".into());
        if screen_commands
            .try_send(Command::StartShare(
                source.to_string(),
                quality.to_string(),
                include_audio && !source.starts_with("camera:"),
                allow_control && !source.starts_with("camera:"),
            ))
            .is_ok()
        {
            ui.set_busy(true);
        } else {
            show_error(&ui, AuthError::Network);
        }
    });
    let screen_commands = commands.clone();
    ui.on_stop_share(move || {
        let _ = screen_commands.try_send(Command::StopShare);
    });
    let target_commands = commands.clone();
    let targets = control_targets.clone();
    let toggle_rc = rc_desktop.clone();
    let toggle_ui = ui.as_weak();
    let toggle_runtime = runtime.handle().clone();
    ui.on_toggle_control(move |peer| {
        let Some(ui) = toggle_ui.upgrade() else {
            return;
        };
        let releasing = toggle_rc.borrow().target(peer.as_str()).is_some();
        if releasing {
            ui.invoke_release_control();
            return;
        }
        if let Some(target) = targets
            .borrow()
            .iter()
            .find(|target| target.peer == peer.as_str())
            .cloned()
        {
            let ticket = target_commands.intents.ticket();
            let request = Arc::new(ViewerRequest::default());
            let target =
                target.with_request(request, ticket, target_commands.intents.voice_token(ticket));
            if toggle_rc.borrow_mut().start(&ui, target.clone()) {
                queue_control_command(
                    &target_commands,
                    &toggle_runtime,
                    Command::ToggleControl(target),
                );
            }
        }
    });
    let weak = ui.as_weak();
    ui.on_control_can_request(move |peer| {
        weak.upgrade()
            .is_some_and(|ui| ui.get_control_permitted().iter().any(|id| id == peer))
    });
    let weak = ui.as_weak();
    let released_rc = rc_desktop.clone();
    let released_authorization = control_authorized.clone();
    let released_since = control_since.clone();
    ui.on_release_control(move || {
        released_rc.borrow_mut().release();
        released_authorization.set(false);
        *released_since.borrow_mut() = None;
        if let Some(ui) = weak.upgrade() {
            ui.set_control_ready(false);
        }
    });
    let revoked_rc = rc_desktop.clone();
    let weak = ui.as_weak();
    ui.on_revoke_control(move || {
        revoked_rc.borrow_mut().revoke_host();
        if let Some(ui) = weak.upgrade() {
            ui.set_control_controller("".into());
        }
    });
    let viewport_rc = rc_desktop.clone();
    let weak = ui.as_weak();
    ui.on_control_viewport(move |peer, x, y, width, height| {
        if let Some(ui) = weak.upgrade() {
            viewport_rc.borrow_mut().layout(
                &ui,
                peer.to_string(),
                [
                    f64::from(x),
                    f64::from(y),
                    f64::from(width),
                    f64::from(height),
                ],
            );
        }
    });
    let confinement_rc = rc_desktop.clone();
    let weak = ui.as_weak();
    ui.on_control_confinement(move |peer, confined| {
        if let Some(ui) = weak.upgrade() {
            confinement_rc
                .borrow_mut()
                .confinement(&ui, peer.as_str(), confined);
        }
    });
    let weak = ui.as_weak();
    let pointer_rc = rc_desktop.clone();
    let control_commands = commands.clone();
    let control_runtime = runtime.handle().clone();
    let pointer_frames = control_frames.clone();
    let pointer_since = control_since.clone();
    let pointer_intents = intents.clone();
    let pointer_generation = video_generation.clone();
    let pointer_stop = stop.clone();
    ui.on_control_pointer(move |peer, x, y, width, height, kind, button| {
        let Some(ui) = weak.upgrade().filter(|ui| {
            ui.get_control_viewer() == peer && ui.get_control_ready() && ui.get_modal() == 0
        }) else {
            return;
        };
        let Some(target) = pointer_rc.borrow().target(peer.as_str()) else {
            return;
        };
        let Some(frame) = pointer_frames.borrow().get(peer.as_str()).cloned() else {
            ui.invoke_release_control();
            return;
        };
        let current = frame.is_current(&pointer_intents, &pointer_generation, &pointer_stop)
            && pointer_since
                .borrow()
                .is_some_and(|since| frame.is_p2p_since(&target, since));
        if !current {
            ui.invoke_release_control();
            return;
        }
        let pixels = frame.stamp.pixels();
        let target = frame.attach(target);
        let point = zabor_native::remote_viewer::normalized_point(x, y, width, height, pixels);
        if let Some((x, y)) = point {
            queue_control_command(
                &control_commands,
                &control_runtime,
                Command::ControlInput(target.clone(), zabor_input::Packet::Move { x, y }),
            );
        }
        if (kind == 2 || (kind == 1 && point.is_some())) && (0..=2).contains(&button) {
            queue_control_command(
                &control_commands,
                &control_runtime,
                Command::ControlInput(
                    target,
                    zabor_input::Packet::Mouse {
                        button: button as u8,
                        down: kind == 1,
                    },
                ),
            );
        }
    });
    let wheel_rc = rc_desktop.clone();
    let control_commands = commands.clone();
    let control_runtime = runtime.handle().clone();
    let weak = ui.as_weak();
    let wheel_frames = control_frames.clone();
    let wheel_since = control_since.clone();
    let wheel_intents = intents.clone();
    let wheel_generation = video_generation.clone();
    let wheel_stop = stop.clone();
    ui.on_control_wheel(move |peer, vertical, horizontal| {
        if !vertical.is_finite() || !horizontal.is_finite() {
            return;
        }
        let Some(ui) = weak.upgrade().filter(|ui| {
            ui.get_control_viewer() == peer && ui.get_control_ready() && ui.get_modal() == 0
        }) else {
            return;
        };
        let target = wheel_rc.borrow().target(peer.as_str());
        if let Some(target) = target {
            let Some(frame) = wheel_frames.borrow().get(peer.as_str()).cloned() else {
                ui.invoke_release_control();
                return;
            };
            if !frame.is_current(&wheel_intents, &wheel_generation, &wheel_stop)
                || !wheel_since
                    .borrow()
                    .is_some_and(|since| frame.is_p2p_since(&target, since))
            {
                ui.invoke_release_control();
                return;
            }
            let target = frame.attach(target);
            queue_control_command(
                &control_commands,
                &control_runtime,
                Command::ControlInput(
                    target,
                    zabor_input::Packet::Wheel {
                        vertical: vertical.clamp(-32768.0, 32767.0) as i16,
                        horizontal: horizontal.clamp(-32768.0, 32767.0) as i16,
                    },
                ),
            );
        }
    });
    let key_rc = rc_desktop.clone();
    let control_commands = commands.clone();
    let control_runtime = runtime.handle().clone();
    let weak = ui.as_weak();
    let key_frames = control_frames.clone();
    let key_since = control_since.clone();
    let key_intents = intents.clone();
    let key_generation = video_generation.clone();
    let key_stop = stop.clone();
    ui.on_control_key(move |peer, text, down, modifiers| {
        let Some(ui) = weak.upgrade().filter(|ui| {
            ui.get_control_viewer() == peer && ui.get_control_ready() && ui.get_modal() == 0
        }) else {
            return;
        };
        let target = key_rc.borrow().target(peer.as_str());
        if let (Some(vk), Some(target)) = (zabor_native::remote_viewer::key_code(&text), target) {
            let Some(frame) = key_frames.borrow().get(peer.as_str()).cloned() else {
                ui.invoke_release_control();
                return;
            };
            if !frame.is_current(&key_intents, &key_generation, &key_stop)
                || !key_since
                    .borrow()
                    .is_some_and(|since| frame.is_p2p_since(&target, since))
            {
                ui.invoke_release_control();
                return;
            }
            let target = frame.attach(target);
            queue_control_command(
                &control_commands,
                &control_runtime,
                Command::ControlInput(
                    target,
                    zabor_input::Packet::Key {
                        vk,
                        down,
                        modifiers: modifiers as u8,
                    },
                ),
            );
        }
    });
    let weak = ui.as_weak();
    let stream_commands = commands.clone();
    let stream_runtime = runtime.handle().clone();
    let focused_rc = rc_desktop.clone();
    ui.on_focus_stream_audio(move |peer| {
        let Some(ui) = weak.upgrade().filter(|ui| ui.get_voice_active()) else {
            return;
        };
        let previous = focused_rc.borrow().target(ui.get_control_viewer().as_str());
        if previous.is_some_and(|target| target.peer != peer.as_str()) {
            ui.invoke_release_control();
        }
        if peer.is_empty() || ui.get_video_frames().iter().any(|video| video.id == peer) {
            if let Err(error) =
                stream_commands.try_send(Command::FocusStreamAudio(peer.to_string()))
            {
                if let mpsc::error::TrySendError::Full(queued) = *error {
                    let sender = stream_commands.sender.clone();
                    let intents = stream_commands.intents.clone();
                    stream_runtime.spawn(async move {
                        let _ = intents
                            .run(queued.ticket, IntentScope::Voice, sender.send(queued))
                            .await;
                    });
                }
            }
        }
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
    let updated_preferences = audio_preferences.clone();
    let update_intents = intents.clone();
    let update_stop = stop.clone();
    let updated_avatars = avatars.clone();
    let updated_app = app_state.clone();
    let updated_members = member_state.clone();
    let control_frame_generation = video_generation.clone();
    let control_frame_stop = stop.clone();
    let updated_rc = rc_desktop.clone();
    ui.on_process_updates(move || {
        if update_stop.is_cancelled() {
            return;
        }
        let Some(ui) = weak.upgrade() else {
            return;
        };
        for _ in 0..32 {
            let Ok(event) = updates_rx.try_recv() else {
                break;
            };
            let Some(event) = event.for_current(&update_intents) else {
                continue;
            };
            match event {
                UiEvent::Guarded(..) | UiEvent::PeerGuarded(..) => unreachable!(),
                UiEvent::Connected => ui.set_connected(true),
                UiEvent::MissingAccount => {
                    ui.set_busy(false);
                    ui.set_auth_step(1);
                }
                UiEvent::SignedIn(data) => {
                    apply_bootstrap(&ui, &data);
                    if let Some(settings) = updated_preferences.settings() {
                        apply_audio_settings(&ui, &settings);
                    }
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
                    ui.set_screen_sources_loading(false);
                    ui.set_camera_sources_loading(false);
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
                UiEvent::Hub(event) => {
                    updated_avatars.borrow_mut().user_event(&ui, &event);
                    apply_hub_event(&ui, &mut latest.borrow_mut(), event);
                }
                UiEvent::Disconnected => {
                    ui.set_connected(false);
                    ui.set_busy(false);
                }
                UiEvent::SignedOut => {
                    updated_rc.borrow_mut().close();
                    control_frames.borrow_mut().clear();
                    control_authorized.set(false);
                    *control_since.borrow_mut() = None;
                    updated_avatars.borrow_mut().cancel(&ui);
                    setup_avatar.borrow_mut().take();
                    ui.set_setup_avatar(slint::Image::default());
                    ui.set_avatar_color(color("#C81E70"));
                    pending_files.borrow_mut().take();
                    *latest.borrow_mut() = Bootstrap::default();
                    ui.set_authenticated(false);
                    ui.set_auth_step(0);
                    ui.set_modal(0);
                    ui.set_password("".into());
                    ui.set_login("".into());
                    ui.set_busy(false);
                    ui.set_channels(ModelRc::default());
                    ui.set_friends(ModelRc::default());
                    ui.set_notifications(ModelRc::default());
                    ui.set_show_notifications(false);
                    ui.set_pending_channel("".into());
                    ui.set_chat_lines(ModelRc::default());
                    ui.set_selected_id("".into());
                    ui.set_user_id("".into());
                    ui.set_chat_input("".into());
                }
                UiEvent::Avatar(event) => {
                    updated_avatars
                        .borrow_mut()
                        .handle(event, &ui, &update_intents, &setup_avatar)
                }
                UiEvent::App(event) => updated_app.borrow_mut().handle(&ui, event, &update_intents),
                UiEvent::Members(event) => updated_members.borrow_mut().handle(
                    &ui,
                    &updated_app,
                    &latest.borrow(),
                    event,
                    &update_intents,
                ),
                UiEvent::ProfileSaved(user, query) => {
                    updated_avatars
                        .borrow_mut()
                        .profile_saved(&ui, &user, &query);
                    if latest.borrow().user.id == user.id {
                        let mut fresh = latest.borrow().user.clone();
                        fresh.display_name = user.display_name;
                        fresh.avatar_base64 = user.avatar_base64;
                        fresh.avatar_color = user.avatar_color;
                        fresh.about_me = user.about_me;
                        let event = HubEvent {
                            target: "UserUpdated".into(),
                            arguments: vec![json!(fresh)],
                        };
                        apply_hub_event(&ui, &mut latest.borrow_mut(), event);
                    }
                }
                UiEvent::ProfileError(query, error) => updated_avatars
                    .borrow_mut()
                    .profile_error(&ui, error, &query),
                UiEvent::ProfileLookup(query, result) => {
                    updated_avatars.borrow_mut().profile_lookup(
                        &ui,
                        &query,
                        result.clone(),
                        &update_intents,
                    );
                    updated_members.borrow_mut().profile_lookup(
                        &ui,
                        &updated_app,
                        &query,
                        result,
                        &update_intents,
                    );
                }
                UiEvent::ProfileFriendResult(query, sent) => {
                    updated_avatars
                        .borrow_mut()
                        .friend_result(&ui, &query, sent, &update_intents);
                    updated_members
                        .borrow_mut()
                        .friend_result(&ui, &query, sent, &update_intents);
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
                    if ui.get_modal() == 4 || ui.get_modal() == 6 {
                        ui.set_modal(0);
                    }
                    ui.set_pending_channel("".into());
                    ui.set_active_channel(channel.into());
                    ui.set_voice_users(friend_rows(&users));
                    apply_bootstrap(&ui, &latest.borrow());
                    ui.set_busy(false);
                }
                UiEvent::VoiceEnded => {
                    updated_rc.borrow_mut().close();
                    control_frames.borrow_mut().clear();
                    control_authorized.set(false);
                    *control_since.borrow_mut() = None;
                    control_targets.borrow_mut().clear();
                    ui.set_control_viewer("".into());
                    ui.set_control_ready(false);
                    ui.set_control_controller("".into());
                    ui.set_control_permitted(ModelRc::default());
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
                    ui.set_screen_sources_loading(false);
                    if ui.get_modal() == 5 {
                        ui.set_modal(0);
                    }
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
                                    .map(|message| chat_line(message, ui.get_user_id().as_str()))
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
                UiEvent::Screens(query, sources) => {
                    if query.is_cancelled()
                        || ui.get_modal() != 5
                        || ui.get_screen_source_kind() == 2
                    {
                        continue;
                    }
                    let Some(sources) = sources else {
                        ui.set_screen_sources_loading(false);
                        show_error(&ui, AuthError::Network);
                        continue;
                    };
                    let strings = Strings::new(ui.get_language().as_str()).ok();
                    let screen_label = strings
                        .as_ref()
                        .map(|s| s.text("stream.screen"))
                        .unwrap_or_default();
                    let window_label = strings
                        .as_ref()
                        .map(|s| s.text("stream.apps"))
                        .unwrap_or_default();
                    let mut screens = Vec::new();
                    let mut windows = Vec::new();
                    for source in sources {
                        let target = match source.kind {
                            SourceKind::Screen => &mut screens,
                            SourceKind::Window => &mut windows,
                        };
                        let row = ScreenItem {
                            id: source.key().into(),
                            title: source
                                .display_title(target.len(), &screen_label, &window_label)
                                .into(),
                            thumbnail: match source.kind {
                                SourceKind::Screen => ui.get_screen_sources(),
                                SourceKind::Window => ui.get_window_sources(),
                            }
                            .iter()
                            .find(|row| row.id == source.key())
                            .map(|row| row.thumbnail)
                            .unwrap_or_default(),
                        };
                        target.push(row);
                    }
                    ui.set_screen_sources(Rc::new(VecModel::from(screens)).into());
                    ui.set_window_sources(Rc::new(VecModel::from(windows)).into());
                    ui.set_screen_sources_loading(false);
                    clear_missing_picker_selection(&ui);
                }
                UiEvent::Cameras(query, sources) => {
                    if !query.is_cancelled()
                        && ui.get_modal() == 5
                        && ui.get_screen_source_kind() == 2
                    {
                        let Some(sources) = sources else {
                            ui.set_camera_sources_loading(false);
                            show_error(&ui, AuthError::Network);
                            continue;
                        };
                        let camera_label = Strings::new(ui.get_language().as_str())
                            .ok()
                            .map(|strings| strings.text("stream.camera"))
                            .unwrap_or_default();
                        let rows = sources
                            .into_iter()
                            .enumerate()
                            .map(|(index, source)| ScreenItem {
                                id: format!("camera:{}", source.id).into(),
                                title: if source.name.is_empty() {
                                    format!("{camera_label} {}", index + 1).into()
                                } else {
                                    source.name.into()
                                },
                                thumbnail: ui
                                    .get_camera_sources()
                                    .iter()
                                    .find(|row| row.id == format!("camera:{}", source.id))
                                    .map(|row| row.thumbnail)
                                    .unwrap_or_default(),
                            })
                            .collect::<Vec<_>>();
                        ui.set_camera_sources(Rc::new(VecModel::from(rows)).into());
                        ui.set_camera_sources_loading(false);
                        clear_missing_picker_selection(&ui);
                    }
                }
                UiEvent::SourcePreview(query, source, image) => {
                    if !query.is_cancelled() && ui.get_modal() == 5 {
                        apply_picker_preview(&ui, &source, image);
                    }
                }
                UiEvent::Sharing(active) => {
                    ui.set_sharing(active);
                    ui.set_busy(false);
                    if ui.get_modal() == 5 {
                        ui.set_modal(0);
                    }
                }
                UiEvent::RemoteControl(mut update) => {
                    if !updated_rc.borrow_mut().update(&ui, &update) {
                        continue;
                    }
                    updated_rc
                        .borrow_mut()
                        .update_host(&ui, update.host.take(), &update.state);
                    let state = update.state;
                    let targets = update.targets;
                    if let Some(key) = state.notice {
                        if let Ok(strings) = Strings::new(ui.get_language().as_str()) {
                            ui.set_error(strings.text(key).into());
                        }
                    }
                    *control_targets.borrow_mut() = targets;
                    control_authorized.set(state.viewer_ready);
                    *control_since.borrow_mut() = state.viewer_accepted_at;
                    let ready = state.viewer_ready
                        && updated_rc.borrow().ready()
                        && state.viewer_accepted_at.is_some_and(|since| {
                            control_targets
                                .borrow()
                                .iter()
                                .find(|target| target.peer == state.viewer)
                                .is_some_and(|target| {
                                    control_frames.borrow().get(&state.viewer).is_some_and(
                                        |frame| {
                                            frame.is_current(
                                                &update_intents,
                                                &control_frame_generation,
                                                &control_frame_stop,
                                            ) && frame.is_p2p_since(target, since)
                                        },
                                    )
                                })
                        });
                    ui.set_control_permitted(
                        Rc::new(VecModel::from(
                            state
                                .permitted_peers
                                .into_iter()
                                .map(slint::SharedString::from)
                                .collect::<Vec<_>>(),
                        ))
                        .into(),
                    );
                    ui.set_control_viewer(state.viewer.into());
                    ui.set_control_ready(ready);
                    ui.set_control_host_active(state.host_active);
                    ui.set_control_controller(state.controller.into());
                }
                UiEvent::ControlActivity(target) => updated_rc.borrow().activity(&target),
                UiEvent::RemoveVideo(peer) => {
                    updated_rc.borrow_mut().remove_video(&peer);
                    control_frames.borrow_mut().remove(&peer);
                    if ui.get_focused_video() == peer {
                        ui.set_focused_video("".into());
                    }
                    let rows = ui
                        .get_video_frames()
                        .iter()
                        .filter(|row| row.id != peer)
                        .collect::<Vec<_>>();
                    ui.set_video_frames(Rc::new(VecModel::from(rows)).into());
                }
                UiEvent::NotificationResolved(kind, id, accept) => {
                    resolve_notification(&ui, &mut latest.borrow_mut(), kind, &id, accept);
                }
                UiEvent::PickedFiles(owner, peer, mut paths) => {
                    if owner == ui.get_user_id().as_str()
                        && latest
                            .borrow()
                            .friends
                            .iter()
                            .any(|friend| friend.id == peer)
                        && !paths.is_empty()
                    {
                        if ui.get_modal() == 7 {
                            if let Some((old_owner, old_peer, old_paths)) =
                                pending_files.borrow_mut().take()
                            {
                                if old_owner == owner && old_peer == peer {
                                    paths.splice(0..0, old_paths);
                                }
                            }
                        }
                        if paths.len() > 20 {
                            if let Ok(strings) = Strings::new(ui.get_language().as_str()) {
                                ui.set_error(strings.text("chat.file.failed").into());
                            }
                            continue;
                        }
                        ui.set_attachment_names(
                            paths
                                .iter()
                                .filter_map(|path| path.file_name())
                                .map(|name| name.to_string_lossy().into_owned())
                                .collect::<Vec<_>>()
                                .join("\n")
                                .into(),
                        );
                        *pending_files.borrow_mut() = Some((owner, peer, paths));
                        ui.set_modal(7);
                    }
                }
                UiEvent::FileProgress(peer, id, progress) => {
                    if ui.get_selected_id() == peer {
                        let model = ui.get_chat_lines();
                        if let Some((index, mut row)) =
                            model.iter().enumerate().find(|(_, row)| row.id == id)
                        {
                            let percent = (progress.clamp(0.0, 1.0) * 100.0) as i32;
                            if percent != row.progress {
                                row.progress = percent;
                                row.transfer = "transferring".into();
                                model.set_row_data(index, row);
                            }
                        }
                    }
                }
                UiEvent::ChooseDestination(peer, id) => {
                    if ui.get_selected_id() == peer {
                        ui.invoke_file_action(peer.into(), id.into(), "save".into());
                    }
                }
                UiEvent::AudioDevices(devices) => {
                    ui.set_audio_devices_failed(devices.is_none());
                    if let (Some(devices), Some(settings)) =
                        (devices, updated_preferences.settings())
                    {
                        apply_audio_devices(&ui, &devices, &settings);
                    }
                }
            }
        }
        reconcile_volume_modal(&ui);
        updated_avatars.borrow_mut().refresh(
            &ui,
            &latest.borrow(),
            setup_avatar.borrow().as_deref(),
            &update_intents,
        );
    });
    let mut preview_scroll = false;
    let motion_preview = matches!(preview.as_deref(), Some("mic-motion" | "mic-motion-frames"));
    let motion_capture = preview.as_deref() == Some("mic-motion-frames");
    let focus_preview = preview.as_deref() == Some("mic-focus");
    let pressed_preview = preview.as_deref() == Some("mic-pressed");
    if let Some(preview) = preview {
        match preview.as_str() {
            "login" => {}
            "confirm" => ui.set_auth_step(1),
            "setup" => ui.set_auth_step(2),
            "home" => {
                ui.set_authenticated(true);
            }
            "audio" | "audio-manual" | "audio-raw" | "audio-ultra" | "audio-deepfilter"
            | "audio-general" | "audio-privacy" | "audio-volume" | "mic-idle" | "mic-arming"
            | "mic-recording" | "mic-playing" | "mic-paused" | "mic-silent" | "mic-failed"
            | "mic-motion" | "mic-motion-frames" | "mic-focus" | "mic-pressed" => {
                apply_bootstrap(&ui, &notification_fixture());
                ui.set_authenticated(true);
                ui.set_connected(true);
                let settings = AudioSettings::from_json(&json!({
                    "micThresholdMode": if preview == "audio-manual" { "manual" } else { "auto" },
                    "noiseSuppression": preview != "audio-raw",
                    "ultraLowLatency": preview == "audio-ultra",
                    "smartNoiseModel": if preview == "audio-deepfilter" { "deepfilter" } else { "rnnoise" },
                }));
                apply_audio_settings(&ui, &settings);
                apply_audio_devices(&ui, &[], &settings);
                ui.set_settings_tab(if preview == "audio-general" {
                    0
                } else if preview == "audio-privacy" {
                    2
                } else {
                    1
                });
                ui.set_modal(3);
                if preview.starts_with("mic-") {
                    ui.set_mic_test(MicTestView {
                        stage: match preview.as_str() {
                            "mic-arming" => "arming",
                            "mic-recording" => "recording",
                            "mic-playing" | "mic-paused" | "mic-silent" => "ready",
                            _ => "idle",
                        }
                        .into(),
                        issue: match preview.as_str() {
                            "mic-silent" => "silent",
                            "mic-failed" => "failed",
                            _ => "",
                        }
                        .into(),
                        playing: ["mic-playing", "mic-silent"].contains(&preview.as_str()),
                        duration: if ["mic-playing", "mic-paused", "mic-silent"]
                            .contains(&preview.as_str())
                        {
                            5.0
                        } else {
                            0.0
                        },
                        position: 1.5,
                    });
                    ui.set_mic_db(-24.0);
                }
                if [
                    "audio-manual",
                    "audio-raw",
                    "audio-ultra",
                    "audio-deepfilter",
                ]
                .contains(&preview.as_str())
                {
                    preview_scroll = true;
                }
                if preview == "audio-volume" {
                    let peer = notification_fixture().requests[0].clone();
                    ui.set_voice_active(true);
                    ui.set_voice_users(friend_rows(std::slice::from_ref(&peer)));
                    ui.set_volume_peer(peer.id.into());
                    ui.set_volume_name(peer.display_name.into());
                    ui.set_peer_volume(160.0);
                    ui.set_modal(8);
                }
            }
            "notifications" => {
                apply_bootstrap(&ui, &notification_fixture());
                ui.set_authenticated(true);
                ui.set_connected(true);
                ui.set_show_notifications(true);
            }
            "files" => {
                let mut data = notification_fixture();
                data.friends = data.requests.clone();
                data.friends[0].is_online = true;
                apply_bootstrap(&ui, &data);
                ui.set_authenticated(true);
                ui.set_connected(true);
                ui.set_active_tab(1);
                ui.set_selected_id(data.friends[0].id.clone().into());
                ui.set_selected_name(data.friends[0].display_name.clone().into());
                ui.set_chat_lines(
                    Rc::new(VecModel::from(
                        file_fixture()
                            .into_iter()
                            .map(|message| chat_line(message, "fixture-owner"))
                            .collect::<Vec<_>>(),
                    ))
                    .into(),
                );
            }
            _ => return Err("Unsupported app preview".into()),
        }
    }
    let capture = Timer::default();
    let preview_scroll_timer = Timer::default();
    if preview_scroll {
        let weak = ui.as_weak();
        preview_scroll_timer.start(
            TimerMode::SingleShot,
            Duration::from_millis(100),
            move || {
                if let Some(ui) = weak.upgrade() {
                    ui.set_settings_scroll_y(-380.0);
                }
            },
        );
    }
    let capture_error = Rc::new(RefCell::new(None));
    if pressed_preview {
        if !reduced_motion || interaction_check {
            return Err("Pressed preview requires reduced motion and no interaction-check".into());
        }
        mic_pressed::start(
            &ui,
            &capture,
            snapshot.ok_or("Pressed evidence directory is required")?,
            capture_error.clone(),
        )?;
    } else if focus_preview {
        if !reduced_motion || interaction_check {
            return Err("Focus preview requires reduced motion and no interaction-check".into());
        }
        mic_focus::start(
            &ui,
            &capture,
            snapshot.ok_or("Focus evidence directory is required")?,
            capture_error.clone(),
        )?;
    } else if motion_preview {
        if reduced_motion || interaction_check {
            return Err(
                "Motion preview requires non-reduced motion and no interaction-check".into(),
            );
        }
        mic_motion::start(
            &ui,
            &capture,
            snapshot.ok_or("Motion evidence directory is required")?,
            capture_error.clone(),
            motion_capture,
        )?;
    } else if interaction_check {
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
                    println!("{}", json!({"nativeMicTextLayout": {"scale": ui.get_mic_test_text_scale(), "windowScale": ui.window().scale_factor()}}));
                    println!("{}", json!({"nativeMicTestCapture": {"stage":ui.get_mic_test().stage.as_str(),"issue":ui.get_mic_test().issue.as_str(),"playing":ui.get_mic_test().playing,"duration":ui.get_mic_test().duration,"position":ui.get_mic_test().position,"panelX":(ui.window().size().width as f32 / ui.window().scale_factor() - 156.0) / 2.0,"panelY":ui.get_mic_test_panel_y(),"panelWidth":156,"panelHeight":36.0 + if ui.get_mic_test().stage == "ready" {27.0} else {0.0} + if ui.get_mic_test().issue.is_empty() {0.0} else {19.75},"physicalDevices":false}}));
                    if let Err(failure) = save_snapshot(&ui, &path) {
                        *error.borrow_mut() = Some(failure.to_string());
                    }
                    let _ = ui.hide();
                }
            },
        );
    }
    let ui_result = ui.run();
    ui.invoke_cancel_picker_query();
    clear_picker_rows(&ui);
    mic_test_ui.borrow_mut().close();
    stop.cancel();
    avatar_timer.stop();
    app_timer.stop();
    member_timer.stop();
    let (avatar_jobs, mut avatar_join_failed) = avatars.borrow_mut().take_jobs(&ui);
    worker.abort();
    let (session_io_result, achievement_io_result, audio_cleanup_result) =
        runtime.block_on(async {
            let _ = worker.await;
            for job in avatar_jobs {
                avatar_join_failed |= job.await.is_err();
            }
            let _ = local_settings_job.await;
            let _ = remote_settings_job.await;
            let audio_runtime_result = audio_runtime_job.await;
            let session_result = session_io.finish().await;
            let achievement_result = achievement_io.finish().await;
            let audio_seal_result = audio_runtime.seal_cleanup();
            let audio_result = audio_runtime.wait_for_cleanup().await;
            (
                session_result,
                achievement_result,
                audio_runtime_result
                    .map_err(|_| zabor_native::audio_runtime::AudioCleanupError::Incomplete)
                    .and(audio_seal_result)
                    .and(audio_result),
            )
        });
    runtime.shutdown_timeout(Duration::from_secs(2));
    ui_result?;
    session_io_result?;
    achievement_io_result?;
    audio_cleanup_result?;
    avatar_join_failed |= avatars.borrow().failed();
    if avatar_join_failed {
        return Err("Avatar worker did not complete normally".into());
    }
    if let Some(error) = capture_error.borrow_mut().take() {
        return Err(error.into());
    }
    Ok(())
}

async fn worker(
    server: String,
    mut commands: mpsc::Receiver<QueuedCommand>,
    update: Arc<dyn Fn(UiEvent) + Send + Sync>,
    stop: CancellationToken,
    profile: Option<Arc<Profile>>,
    media: WorkerMedia,
) {
    let WorkerMedia {
        video_sink,
        video_generation,
        audio_preferences,
        audio_runtime,
        intents,
        session_io,
        achievement_io,
    } = media;
    let chat_io = session_io.chat_io.clone();
    let file_io = session_io.file_io.clone();
    let audio_controls = audio_preferences.controls.clone();
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
    let mut saved_credentials = None;
    let mut retry_at = None;
    let mut retry_attempt = 0_u32;
    let mut session_ready = false;
    let mut voice: Option<VoiceSession> = None;
    let mut media_events: Option<mpsc::Receiver<MediaEvent>> = None;
    let mut voice_ticket: Option<IntentTicket> = None;
    let mut chat_resources = file_io.own();
    let (chat, chat_transport, file_jobs) = chat_resources.split();
    let mut file_progress = BTreeMap::<(String, String), i32>::new();
    let mut file_watch = tokio::time::interval(Duration::from_secs(1));
    file_watch.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut chat_ice = Vec::<zabor_native::media::IceServerSettings>::new();
    let mut chat_failed = false;
    let mut friends = Vec::<String>::new();
    let mut selected_chat: Option<String> = None;
    let mut prune_at = tokio::time::Instant::now() + Duration::from_secs(3600);
    let mut incoming_call: Option<String> = None;
    let mut call_connected = false;
    let mut call_recovery = CallRecovery::default();
    let mut abandoned_call = false;
    let mut pending_stop_stream = false;
    let mut video_jobs = BTreeMap::<String, VideoReceiver>::new();
    let mut paused_video = BTreeSet::<String>::new();
    let mut stream_drops = StreamDropGrace::default();
    let mut share_watch = tokio::time::interval(Duration::from_secs(1));
    share_watch.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut stream_watch = tokio::time::interval(Duration::from_millis(250));
    stream_watch.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut screen_sources = Vec::<ScreenSource>::new();
    let mut camera_sources = Vec::<zabor_camera::Device>::new();
    let mut pending_join: Option<String> = None;
    let mut audio_watch = tokio::time::interval(Duration::from_millis(50));
    audio_watch.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut sent_audio_state = None;
    let mut sent_speaking = false;
    let mut speaking_release = None;
    let mut local_speaking = false;
    macro_rules! voice_leave_state {
        () => {
            VoiceLeave {
                voice: &mut voice,
                media_events: &mut media_events,
                voice_ticket: &mut voice_ticket,
                pending_join: &mut pending_join,
                video_jobs: &mut video_jobs,
                paused_video: &mut paused_video,
                stream_drops: &mut stream_drops,
                call_recovery: &mut call_recovery,
                abandoned_call: &mut abandoned_call,
                pending_stop_stream: &mut pending_stop_stream,
                call_connected: &mut call_connected,
                sent_audio_state: &mut sent_audio_state,
                sent_speaking: &mut sent_speaking,
                speaking_release: &mut speaking_release,
                local_speaking: &mut local_speaking,
                video_generation: &video_generation,
                audio_demand: &audio_runtime.demand,
            }
        };
    }
    macro_rules! run_session {
        ($work:expr, $future:expr) => {
            $work
                .run_with_leave($future, &mut voice_leave_state!(), hub.as_ref())
                .await
        };
    }
    let startup_work = SessionWork::new(&intents, intents.ticket(), &stop, update.as_ref());
    if let Some(Ok(Some(session))) = run_session!(startup_work, session_io.load()) {
        startup_work.commit(|| {
            saved_credentials = Some(Credentials {
                username: session.username,
                password: session.password,
            });
            retry_at = Some(tokio::time::Instant::now());
        });
    }
    loop {
        if stop.is_cancelled() {
            break;
        }
        let iteration_ticket = intents.ticket();
        let session_work = SessionWork::new(&intents, iteration_ticket, &stop, update.as_ref());
        if voice.is_none() {
            let work = VoiceWork::new(
                &intents,
                voice_ticket.unwrap_or(iteration_ticket),
                &stop,
                update.as_ref(),
            );
            let retain_events = media_events.is_some();
            let clear_video = !video_jobs.is_empty()
                || !paused_video.is_empty()
                || stream_drops.next_deadline().is_some();
            let retired = work.take_idle_media(
                &mut voice,
                &mut media_events,
                &mut video_jobs,
                voice_ticket,
                || {
                    if !retain_events {
                        voice_ticket = None;
                    }
                    call_recovery.clear();
                    sent_audio_state = None;
                    sent_speaking = false;
                    speaking_release = None;
                    local_speaking = false;
                    audio_controls.speaking.store(false, Ordering::Release);
                    if clear_video {
                        video_generation.fetch_add(1, Ordering::AcqRel);
                        paused_video.clear();
                        stream_drops.clear();
                    }
                },
            );
            drop(retired);
        }
        if session_work.is_current() && chat.is_none() && !chat_failed {
            if let (Some(user), Some(profile)) = (current_user.as_ref(), profile.as_ref()) {
                let initialized = run_session!(
                    session_work,
                    chat_io.before_store(|| initialize_chat(profile, &user.id, &friends))
                );
                let Some(initialized) = initialized else {
                    continue;
                };
                match initialized {
                    Ok(engine) => {
                        *chat = Some(engine);
                        *file_jobs = Some(file_io::FileIo::bound(
                            &profile.root().join("chat-files"),
                            intents.clone(),
                            iteration_ticket,
                            stop.clone(),
                        ));
                        *chat_transport = ChatTransport::with_stop(
                            user.id.clone(),
                            friends.iter().cloned(),
                            chat_ice.clone(),
                            audio_preferences.relay_only(),
                            intents.session_token(iteration_ticket),
                        )
                        .ok();
                    }
                    Err(_) => {
                        chat_failed = true;
                        session_work.publish(UiEvent::Toast("chat.storageFailed"));
                    }
                }
            }
        }
        run_session!(session_work, async {
            if let (Some(jobs), Some(engine)) = (file_jobs.as_mut(), chat.as_mut()) {
                file_progress.retain(|(peer, id), _| engine.is_downloading(peer, id));
                let packets = jobs.pump(engine, chat_transport.as_ref(), now_ms());
                if let Some(hub) = &hub {
                    send_chat_packets(&session_work, hub, chat_transport.as_ref(), packets).await;
                }
            }
        });
        if session_work.is_current() && chat_transport.is_none() && session_ready {
            if let Some(user) = &current_user {
                *chat_transport = ChatTransport::with_stop(
                    user.id.clone(),
                    friends.iter().cloned(),
                    chat_ice.clone(),
                    audio_preferences.relay_only(),
                    intents.session_token(iteration_ticket),
                )
                .ok();
            }
        }
        let file_tick = chat.as_ref().is_some_and(ChatEngine::has_downloads)
            || file_jobs.as_ref().is_some_and(FileJobs::needs_tick);
        let mut command_ticket = iteration_ticket;
        let mut command = tokio::select! {
            biased;
            _ = stop.cancelled() => break,
            action = intents.next() => { command_ticket = intents.ticket(); match action { CriticalAction::Leave => Command::Leave, CriticalAction::Logout => Command::Logout } },
            command = commands.recv() => { let Some(queued) = command else { break; }; if !intents.is_current(queued.ticket, queued.command.scope()) { continue; } command_ticket = queued.ticket; queued.command },
            _ = async { if voice.is_some() { audio_watch.tick().await; } else { std::future::pending::<()>().await; } } => Command::CheckAudio,
            _ = async { if file_tick { file_watch.tick().await; } else { std::future::pending::<()>().await; } } => Command::MaintainFiles,
            event = async { match file_jobs.as_mut().filter(|jobs| jobs.has_jobs()) { Some(jobs) => jobs.next_event().await, None => std::future::pending().await } } => {
                run_session!(session_work, async {
                let update = |event| session_work.publish(event);
                if let (Some(event), Some(engine)) = (event, chat.as_mut()) {
                    let peer = match event {
                        FileEvent::Staged { peer, file } => {
                            match file {
                                Ok(file) => match engine.send_file(&peer, file.info().clone(), now_ms()) {
                                    Ok(packets) => { file.commit(); if let Some(hub) = &hub { send_chat_packets(&session_work, hub, chat_transport.as_ref(), packets).await; } },
                                    Err(_) => update(UiEvent::Toast("chat.file.failed")),
                                },
                                Err(_) => update(UiEvent::Toast("chat.file.failed")),
                            }
                            peer
                        },
                        FileEvent::Sent { peer, id, success, .. } => {
                            if !success { if let (Some(hub), Ok(packet)) = (hub.as_ref(), engine.unavailable(&peer, &id)) { send_chat_packets(&session_work, hub, chat_transport.as_ref(), vec![packet]).await; } }
                            peer
                        },
                        FileEvent::Saved { peer, id, stored_name, path } => {
                            match path { Ok(path) => { if engine.saved_file(&peer, &id, &stored_name, &path, now_ms()).is_err() { update(UiEvent::Toast("chat.storageFailed")); } }, Err(_) => update(UiEvent::Toast("chat.file.failed")) }
                            peer
                        },
                        FileEvent::Revealed { peer, id, success } => {
                            if !success { let _ = engine.forget_saved_path(&peer, &id, now_ms()); update(UiEvent::ChooseDestination(peer.clone(), id)); }
                            peer
                        },
                    };
                    if selected_chat.as_deref() == Some(&peer) { if let Ok(messages) = engine.messages(&peer, now_ms()) { update(UiEvent::ChatMessages(peer, messages, false)); } }
                }
                });
                continue;
            },
            channel = async { match pending_join.take() { Some(channel) => channel, None => std::future::pending().await } } => Command::Join(channel),
            _ = async { if let Some(deadline) = stream_drops.next_deadline() { tokio::time::sleep_until(deadline.into()).await; } else { std::future::pending::<()>().await; } } => Command::ExpireStreams,
            _ = async { if voice.as_ref().is_some_and(VoiceSession::is_sharing) { share_watch.tick().await; } else { std::future::pending::<()>().await; } } => Command::CheckShare,
            _ = async { if voice.is_some() && session_ready { stream_watch.tick().await; } else { std::future::pending::<()>().await; } } => Command::MaintainStreams,
            _ = tokio::time::sleep_until(prune_at) => { prune_at = tokio::time::Instant::now() + Duration::from_secs(3600); Command::PruneChat },
            _ = async { if let Some(deadline) = call_recovery.next_deadline() { tokio::time::sleep_until(deadline.into()).await; } else { std::future::pending::<()>().await; } } => Command::ResumeCall,
            _ = async { match retry_at { Some(deadline) => tokio::time::sleep_until(deadline).await, None => std::future::pending().await } } => { retry_at = None; Command::Reconnect },
            event = async { match events.as_mut() { Some(events) => events.recv().await, None => std::future::pending().await } } => {
                match event {
                    Some(event) => {
                        let event_ticket = command_ticket;
                        let voice_work = voice_ticket.map(|ticket| VoiceWork::new(&intents, ticket, &stop, update.as_ref()));
                        if let Some(user) = current_user.as_mut() {
                            zabor_native::roster::apply_user_event(user, &event, true);
                            audio_controls.set_server_state(user.is_server_muted, user.is_server_deafened);
                        }
                        if event.target == "StreamDropped" && voice_work.as_ref().is_some_and(VoiceWork::is_current) {
                            if let (Some(work), Some(peer)) = (voice_work.as_ref(), event.arguments.first().and_then(serde_json::Value::as_str).filter(|peer| voice.as_ref().is_some_and(|voice| voice.has_peer(peer)))) {
                                let lifetime = video_jobs.get(peer).map(|receiver| receiver.lifetime().clone());
                                work.take_video(&mut video_jobs, voice_ticket, lifetime.as_ref(), peer, |_| { stream_drops.schedule(peer.into(), std::time::Instant::now()); false });
                            }
                        }
                        if event.target == "UserStoppedStreaming" {
                            if let (Some(work), Some(peer)) = (voice_work.as_ref().filter(|work| work.is_current()), event.arguments.first().and_then(serde_json::Value::as_str).filter(|peer| voice.as_ref().is_some_and(|voice| voice.has_peer(peer)))) {
                                let lifetime = video_jobs.get(peer).map(|receiver| receiver.lifetime().clone());
                                if let Some(receiver) = work.take_video(&mut video_jobs, voice_ticket, lifetime.as_ref(), peer, |_| { paused_video.insert(peer.into()); true }) {
                                    stream_drops.cancel(peer);
                                    if let Some(mut receiver) = receiver {
                                        receiver.stop();
                                        let replaced = work.install_video(&mut video_jobs, voice_ticket, lifetime.as_ref().unwrap(), (peer.into(), receiver), || {});
                                        drop(replaced);
                                    }
                                    work.publish(UiEvent::RemoveVideo(peer.into()));
                                }
                            }
                        }
                        if event.target == "UserStartedStreaming" {
                            if let (Some(work), Some(peer)) = (voice_work.as_ref().filter(|work| work.is_current()), event.arguments.first().and_then(serde_json::Value::as_str).filter(|peer| voice.as_ref().is_some_and(|voice| voice.has_peer(peer)))) {
                                let lifetime = video_jobs.get(peer).map(|receiver| receiver.lifetime().clone());
                                if let Some(receiver) = work.take_video(&mut video_jobs, voice_ticket, lifetime.as_ref(), peer, |videos| { paused_video.remove(peer); videos.values().filter(|receiver| receiver.is_playing()).count() < 3 }) {
                                    stream_drops.cancel(peer);
                                    if let Some(mut receiver) = receiver {
                                        receiver.start();
                                        let replaced = work.install_video(&mut video_jobs, voice_ticket, lifetime.as_ref().unwrap(), (peer.into(), receiver), || {});
                                        drop(replaced);
                                    }
                                }
                            }
                        }
                        if matches!(event.target.as_str(), "UserLeft" | "UserDeleted") {
                            if let (Some(work), Some(peer)) = (voice_work.as_ref().filter(|work| work.is_current()), event.arguments.first().and_then(serde_json::Value::as_str)) {
                                if let Some(retired) = work.retire_videos(&mut video_jobs, voice_ticket, |id, _| id == peer) {
                                    drop(retired);
                                    if work.commit_resource(voice_ticket, || { if video_jobs.contains_key(peer) { false } else { paused_video.remove(peer); true } }) == Some(true) {
                                        stream_drops.cancel(peer);
                                        work.publish(UiEvent::RemoveVideo(peer.into()));
                                    }
                                }
                            }
                        }
                        if matches!(event.target.as_str(), "FriendAdded" | "FriendRequestAccepted" | "FriendRemoved" | "UserDeleted") {
                            let previous = session_work.commit(|| {
                                let previous = friends.clone();
                                if matches!(event.target.as_str(), "FriendAdded" | "FriendRequestAccepted") {
                                    if let Some(user) = event.arguments.first().and_then(|value| serde_json::from_value::<zabor_native::auth::User>(value.clone()).ok()) { if !user.id.is_empty() && current_user.as_ref().is_none_or(|owner| owner.id != user.id) && !friends.contains(&user.id) { friends.push(user.id); } }
                                } else if let Some(id) = event.arguments.first().and_then(serde_json::Value::as_str) { friends.retain(|friend| friend != id); if selected_chat.as_deref() == Some(id) { selected_chat = None; } }
                                previous
                            });
                            let Some(previous) = previous else { continue; };
                            if run_session!(session_work, chat_io::reconcile(&session_work, &chat_io, chat, chat_transport, file_jobs, &previous, &friends)).is_none() { continue; }
                        }
                        if let (Some(transport), Some(hub), Some(peer), Some(payload)) = (chat_transport.as_mut(), hub.as_ref(), event.arguments.first().and_then(serde_json::Value::as_str), event.arguments.get(1).and_then(serde_json::Value::as_str)) {
                            if payload.starts_with("zabor-chat:") && run_session!(session_work, transport.handle_signal(hub, &event.target, peer, payload)).is_none() { continue; }
                        }
                        match event.target.as_str() {
                            "IncomingCall" => {
                                if let Some(call) = event.arguments.first() {
                                    if let Some(id) = call["callerId"].as_str() {
                                        incoming_call = Some(id.into());
                                        session_work.publish(UiEvent::IncomingCall(id.into(), call["callerName"].as_str().unwrap_or("").into()));
                                    }
                                }
                            },
                            "CallAccepted" | "CallStarted" => {
                                if let (Some(session), Some(hub), Some(value)) = (voice.as_mut(), hub.as_ref(), event.arguments.first()) {
                                    if let Ok(user) = serde_json::from_value::<zabor_native::auth::User>(value.clone()) {
                                        if session.call_peer.as_deref() == Some(user.id.as_str()) {
                                            let Some(ticket) = voice_ticket.filter(|ticket| intents.is_current(*ticket, IntentScope::Voice)) else { continue; };
                                            let work = VoiceWork::new(&intents, ticket, &stop, update.as_ref());
                                            if work.commit_resource(voice_ticket, || { call_connected = true; call_recovery.connected(&user.id); incoming_call = None; }).is_none() { continue; }
                                            if event.target == "CallAccepted" {
                                                let result = tokio::select! { _ = stop.cancelled() => break, result = intents.run(ticket, IntentScope::Voice, session.connect_call(hub, &user.id)) => result };
                                                if result.is_none() { continue; }
                                            }
                                            update(UiEvent::Voice(String::new(), vec![user]).guarded(ticket, IntentScope::Voice));
                                        }
                                    }
                                }
                            },
                            "CallSession" => {
                                if let (Some(work), Some(session)) = (voice_work.as_ref(), event.arguments.first().and_then(Value::as_str)) {
                                    work.commit_resource(voice_ticket, || call_recovery.set_session(session));
                                }
                            },
                            "CallSuspended" => {
                                if let Some(work) = voice_work.as_ref() {
                                if let (Some(session), Some(remaining)) = (event.arguments.first().and_then(Value::as_str), event.arguments.get(1).and_then(Value::as_f64).filter(|value| value.is_finite())) {
                                    work.commit_resource(voice_ticket, || call_recovery.suspended(session, remaining.clamp(0.0, 45_000.0) as u64, std::time::Instant::now()));
                                }
                                }
                            },
                            "CallResumed" => {
                                if let (Some(session), Some(hub), Some(user), Some(id)) = (voice.as_mut(), hub.as_ref(), event.arguments.first().and_then(|value| serde_json::from_value::<zabor_native::auth::User>(value.clone()).ok()), event.arguments.get(1).and_then(Value::as_str)) {
                                    let now = std::time::Instant::now();
                                    if call_recovery.can_restore(id, &user.id, now) {
                                        let Some(ticket) = voice_ticket.filter(|ticket| intents.is_current(*ticket, IntentScope::Voice)) else { continue; };
                                        let result = tokio::select! { _ = stop.cancelled() => break, result = intents.run(ticket, IntentScope::Voice, tokio::time::timeout(call_recovery.remaining(now).min(Duration::from_secs(5)), session.resume_call(hub, &user.id, chat_ice.clone()))) => result };
                                        let Some(result) = result else { continue; };
                                        let work = VoiceWork::new(&intents, ticket, &stop, update.as_ref());
                                        let mut retired_video = None;
                                        let Some(restored) = work.commit_resource(voice_ticket, || {
                                            if matches!(result, Ok(Ok(()))) && call_recovery.restored(id, &user.id, std::time::Instant::now()) {
                                                retired_video = video_jobs.remove(&user.id);
                                                paused_video.remove(&user.id);
                                                true
                                            } else { call_recovery.retry(std::time::Instant::now()); false }
                                        }) else { continue; };
                                        drop(retired_video);
                                        if restored {
                                            stream_drops.cancel(&user.id);
                                            work.publish(UiEvent::RemoveVideo(user.id.clone()));
                                            work.publish(UiEvent::Voice(String::new(), vec![user]));
                                            if matches!(work.run(session.restore_share(hub)).await, Some(Err(_))) {
                                                work.publish(UiEvent::Toast("toasts.streamStartFailed"));
                                            }
                                        }
                                    }
                                }
                            },
                            "CallEnded" | "CallDeclined" if event.target != "CallEnded" || call_recovery.accepts_end(event.arguments.first().and_then(Value::as_str)) => {
                                if voice_work.as_ref().is_some_and(VoiceWork::is_current) && voice.as_ref().is_some_and(|session| session.call_peer.is_some()) && (event.target != "CallDeclined" || !call_connected) {
                                    if let Some(work) = &voice_work {
                                        let Some(disposed) = work.take_voice(&mut voice, &mut media_events, voice_ticket, |_| { call_connected = false; incoming_call = None; }) else { continue; };
                                        drop(disposed);
                                        work.publish(UiEvent::VoiceEnded);
                                    }
                                }
                                else if incoming_call.take().is_some() { session_work.publish(UiEvent::IncomingDismissed); }
                            },
                            _ => {},
                        }
                        if event.target == "ReceiveChatRelay" && run_session!(session_work, async {
                            let update = |event| session_work.publish(event);
                            if let (Some(engine), Some(hub), Some(peer), Some(payload)) = (chat.as_mut(), hub.as_ref(), event.arguments.first().and_then(serde_json::Value::as_str), event.arguments.get(1).and_then(serde_json::Value::as_str)) {
                                match engine.receive(peer, payload, now_ms()) {
                                    Ok(packets) => {
                                        send_chat_packets(&session_work, hub, chat_transport.as_ref(), packets).await;
                                        if selected_chat.as_deref() == Some(peer) {
                                            if let Ok(packets) = engine.mark_read(peer, now_ms()) { send_chat_packets(&session_work, hub, chat_transport.as_ref(), packets).await; }
                                            if let Ok(messages) = engine.messages(peer, now_ms()) { update(UiEvent::ChatMessages(peer.into(), messages, false)); }
                                        }
                                    },
                                    Err(ChatError::Identity) => update(UiEvent::Toast("chat.keyChanged")),
                                    Err(_) => {},
                                }
                            }
                            }).is_none() { continue; }
                        if let (Some(voice), Some(hub), Some(ticket)) = (voice.as_mut(), hub.as_ref(), voice_ticket) {
                            let result = tokio::select! { _ = stop.cancelled() => break, result = intents.run(ticket, IntentScope::Voice, voice.handle(hub, &event.target, &event.arguments)) => result };
                            if result.is_none() { continue; }
                        }
                        if let Some(work) = voice_work.as_ref().filter(|work| work.is_current()) {
                            let Some(retired) = work.retire_videos(&mut video_jobs, voice_ticket, |_, receiver| !receiver.is_alive()) else { continue; };
                            for (peer, receiver) in retired { stream_drops.cancel(&peer); if !voice.as_ref().is_some_and(|voice| voice.has_stream_snapshot(&peer)) { work.publish(UiEvent::RemoveVideo(peer)); } drop(receiver); }
                            if work.commit_resource(voice_ticket, || paused_video.retain(|peer| voice.as_ref().is_some_and(|voice| voice.has_peer(peer)))).is_none() { continue; }
                            if event.target == "ForceLeaveVoice" || voice.as_ref().is_some_and(|voice| voice.channel.is_none() && voice.call_peer.is_none()) {
                                let Some(disposed) = work.take_voice(&mut voice, &mut media_events, voice_ticket, |_| {}) else { continue; };
                                drop(disposed);
                                work.publish(UiEvent::VoiceEnded);
                            }
                        }
                        update(UiEvent::Hub(event).guarded(event_ticket, IntentScope::Session));
                    },
                    None => {
                        if let Some(ticket) = voice_ticket {
                            VoiceWork::new(&intents, ticket, &stop, update.as_ref()).commit_resource(voice_ticket, || call_recovery.disconnected(std::time::Instant::now()));
                        }
                        audio_preferences.disconnect();
                        events = None; hub = None; session_ready = false; *chat_transport = None;
                        if let Some(engine) = chat.as_mut() { engine.fail_downloads(None, now_ms()); }
                        if let Some(jobs) = file_jobs.as_mut() { for peer in &friends { jobs.disconnect(peer); } }
                        if saved_credentials.is_some() { retry_at = Some(tokio::time::Instant::now() + Duration::from_secs(2)); } update(UiEvent::Disconnected);
                    }
                }
                continue;
            },
            event = async { match media_events.as_mut() { Some(events) => events.recv().await, None => std::future::pending().await } } => {
                let Some(event) = event else { media_events = None; continue; };
                let Some(ticket) = voice_ticket else { continue; };
                let work = VoiceWork::new(&intents, ticket, &stop, update.as_ref());
                let lifetime = event.lifetime().clone();
                work.run_peer(&lifetime, async {
                match event {
                    MediaEvent::Ice { peer, payload, .. } => { if let Some(hub) = &hub { let _ = work.send_peer(&lifetime, hub, "SendIceCandidate", vec![json!(peer), json!(payload)]).await; } },
                    MediaEvent::Renegotiate { peer, .. } => {
                        if let (Some(voice), Some(hub)) = (voice.as_mut(), hub.as_ref()) {
                            let _ = voice.renegotiate(hub, &peer).await;
                        }
                    },
                    MediaEvent::Track { peer, track: MediaStreamTrack::Audio(track), lifetime, stream } => {
                        if let Some(voice) = voice.as_mut() { if voice.receive_audio(&peer, track, lifetime.clone(), stream).is_err() { work.publish_peer(&lifetime, UiEvent::Toast("toasts.micUnknownError")); } }
                    },
                    MediaEvent::Track { peer, track: MediaStreamTrack::Video(track), lifetime, .. } => {
                        if let Some(voice) = voice.as_mut() { voice.prepare_p2p_video(&peer, track, lifetime); }
                    },
                    event @ (MediaEvent::SfuConnection { .. } | MediaEvent::SfuTrack { .. }) => {
                        if let (Some(voice), Some(hub)) = (voice.as_mut(), hub.as_ref()) { let _ = voice.handle_sfu_event(hub, event).await; }
                    },
                    MediaEvent::RemoteControl { peer, event, lifetime, channel } => {
                        if let (Some(voice), Some(hub)) = (voice.as_mut(), hub.as_ref()) {
                            voice.bind_control_intents(intents.clone(), ticket);
                            let original = voice.control_viewer_target();
                            let _ = voice.handle_remote_control(hub, &peer, event, lifetime.clone(), channel, intents.voice_token(ticket)).await;
                            work.publish_peer(&lifetime, UiEvent::RemoteControl(Box::new(rc_desktop::Update::from_session(voice, original))));
                        }
                    },
                    MediaEvent::StreamVideoReady { peer, epoch, track, lifetime, lease } => {
                            let accepted = if let (Some(voice), Some(hub)) = (voice.as_mut(), hub.as_ref()) { voice.accept_stream_video_ready(hub, &peer, epoch, &lifetime, &lease).await } else { false };
                            if !accepted { return; }
                            let sink = video_sink.clone();
                            let generation = video_generation.load(Ordering::Acquire);
                            let id = peer.clone();
                            let mut receiver = VideoReceiver::with_audio_runtime(track, lifetime.clone(), Arc::new(move |delivery| { sink(FrameUpdate { peer: id.clone(), ticket, generation, lease: delivery.lease, frame: delivery.frame, source_epoch: epoch }); }), audio_runtime.as_ref()).with_source_epoch(epoch);
                            let mut retired_video = None;
                            let Some(watch) = work.commit_peer_resource(voice_ticket, &lifetime, || {
                                retired_video = video_jobs.remove(&peer);
                                !paused_video.contains(&peer) && video_jobs.values().filter(|receiver| receiver.is_playing()).count() < 3
                            }) else { return; };
                            drop(retired_video);
                            if watch {
                                receiver.start();
                            }
                            let replaced = work.install_video(&mut video_jobs, voice_ticket, &lifetime, (peer, receiver), || {});
                            drop(replaced);
                    },
                    _ => {},
                }
                }).await;
                continue;
            },
            event = async { match chat_transport.as_mut() { Some(transport) => transport.next_event().await, None => std::future::pending().await } } => {
                run_session!(session_work, async {
                let update = |event| session_work.publish(event);
                if let (Some(event), Some(hub), Some(chat)) = (event, hub.as_ref(), chat.as_mut()) {
                    match event {
                        ChatTransportEvent::Ice(peer, payload) => { let _ = hub.send("SendIceCandidate", vec![json!(peer), json!(payload)]).await; },
                        ChatTransportEvent::Ready(peer) => { if let Ok(packet) = chat.open(&peer) { send_chat_packets(&session_work, hub, chat_transport.as_ref(), vec![packet]).await; } },
                        ChatTransportEvent::Text(peer, payload) => {
                            if let Ok(packets) = chat.receive_direct(&peer, &payload, now_ms()) {
                                send_chat_packets(&session_work, hub, chat_transport.as_ref(), packets).await;
                                if selected_chat.as_deref() == Some(peer.as_str()) {
                                    if let Ok(packets) = chat.mark_read(&peer, now_ms()) { send_chat_packets(&session_work, hub, chat_transport.as_ref(), packets).await; }
                                    if let Ok(messages) = chat.messages(&peer, now_ms()) { update(UiEvent::ChatMessages(peer, messages, false)); }
                                }
                            }
                        },
                        ChatTransportEvent::Binary(peer, frame) => {
                            match chat.receive_file(&peer, &frame, now_ms()) {
                                Ok((id, progress, complete)) => {
                                    if complete {
                                        if let Some(jobs) = file_jobs.as_mut() { if jobs.finish_incoming(chat, &peer, &id, now_ms()).is_err() { update(UiEvent::Toast("chat.file.failed")); } }
                                        if let Ok(messages) = chat.messages(&peer, now_ms()) { update(UiEvent::ChatMessages(peer, messages, false)); }
                                    } else {
                                        let percent = (progress * 100.0) as i32;
                                        if file_progress.insert((peer.clone(), id.clone()), percent) != Some(percent) { update(UiEvent::FileProgress(peer, id, progress)); }
                                    }
                                },
                                Err(ChatError::Storage) => { update(UiEvent::Toast("chat.file.failed")); if let Ok(messages) = chat.messages(&peer, now_ms()) { update(UiEvent::ChatMessages(peer, messages, false)); } },
                                Err(_) => {},
                            }
                        },
                        ChatTransportEvent::FilesReady(_) => {},
                        ChatTransportEvent::Unavailable(peer) => {
                            if let Some(jobs) = file_jobs.as_mut() { jobs.disconnect(&peer); }
                            let packets = chat.fail_downloads(Some(&peer), now_ms());
                            send_chat_packets(&session_work, hub, chat_transport.as_ref(), packets).await;
                            if let Ok(messages) = chat.messages(&peer, now_ms()) { update(UiEvent::ChatMessages(peer, messages, false)); }
                        },
                    }
                }
                });
                continue;
            },
        };
        if matches!(command, Command::CheckAudio) {
            let Some(ticket) = voice_ticket else {
                continue;
            };
            let work = VoiceWork::new(&intents, ticket, &stop, update.as_ref());
            if !work.is_current() {
                continue;
            }
            if voice.as_mut().is_some_and(VoiceSession::audio_failed) {
                let Some(disposed) =
                    work.take_voice(&mut voice, &mut media_events, voice_ticket, |session| {
                        abandoned_call |= session.is_some_and(|session| session.call_peer.is_some())
                    })
                else {
                    continue;
                };
                if let Some(mut session) = disposed.0 {
                    if let Some(hub) = &hub {
                        let result = work.run(session.leave(hub)).await;
                        if result.is_none() {
                            continue;
                        }
                        if work.commit(|| abandoned_call = false).is_none() {
                            continue;
                        }
                    }
                }
                drop(disposed.1);
                work.publish(UiEvent::VoiceEnded);
                work.publish(UiEvent::Toast("toasts.micUnknownError"));
                continue;
            }
            let state = audio_controls.user_state();
            if sent_audio_state != Some(state) {
                if let (Some(session), Some(hub)) = (voice.as_mut(), hub.as_ref()) {
                    let result = work.run(session.mute(hub, state.0, state.1)).await;
                    let Some(result) = result else {
                        continue;
                    };
                    if result.is_ok()
                        && work
                            .commit_resource(voice_ticket, || sent_audio_state = Some(state))
                            .is_none()
                    {
                        continue;
                    }
                }
            }
            let speaking = audio_controls.speaking.load(Ordering::Acquire)
                && !audio_controls.effectively_muted();
            let Some(speaking_user) = work.commit_resource(voice_ticket, || {
                let mut speaking_user = None;
                if local_speaking != speaking {
                    local_speaking = speaking;
                    if let Some(user) = &mut current_user {
                        user.is_speaking = speaking;
                        speaking_user = Some(user.id.clone());
                    }
                }
                speaking_user
            }) else {
                continue;
            };
            if let Some(user) = speaking_user {
                work.publish(UiEvent::Hub(HubEvent {
                    target: "UserSpeaking".into(),
                    arguments: vec![json!(user), json!(speaking)],
                }));
            }
            let now = tokio::time::Instant::now();
            let Some(target) = work.commit_resource(voice_ticket, || {
                if speaking {
                    speaking_release = None;
                } else if sent_speaking && speaking_release.is_none() {
                    speaking_release = Some(now + Duration::from_millis(350));
                }
                speaking
                    || (sent_speaking && speaking_release.is_some_and(|deadline| now < deadline))
            }) else {
                continue;
            };
            if target != sent_speaking {
                if let Some(hub) = &hub {
                    let result = work
                        .run(hub.send("SetSpeakingState", vec![json!(target)]))
                        .await;
                    let Some(result) = result else {
                        continue;
                    };
                    if result.is_ok() {
                        work.commit_resource(voice_ticket, || sent_speaking = target);
                    }
                }
            }
            continue;
        }
        if matches!(command, Command::MaintainFiles) {
            let session_work = SessionWork::new(&intents, command_ticket, &stop, update.as_ref());
            run_session!(session_work, async {
                let update = |event| session_work.publish(event);
                if let Some(engine) = chat.as_mut() {
                    let packets = engine.expire_downloads(now_ms());
                    if let Some(hub) = &hub {
                        send_chat_packets(&session_work, hub, chat_transport.as_ref(), packets)
                            .await;
                    }
                    if let Some(peer) = &selected_chat {
                        if let Ok(messages) = engine.messages(peer, now_ms()) {
                            update(UiEvent::ChatMessages(peer.clone(), messages, false));
                        }
                    }
                }
            });
            continue;
        }
        if let Command::File {
            owner,
            peer,
            action,
        } = command
        {
            let session_work = SessionWork::new(&intents, command_ticket, &stop, update.as_ref());
            run_session!(session_work, async {
                let update = |event| session_work.publish(event);
                if let (Some(engine), Some(jobs)) = (
                    chat.as_mut()
                        .filter(|engine| engine.owner() == owner && engine.is_friend(&peer)),
                    file_jobs.as_mut(),
                ) {
                    let result: Result<Vec<zabor_native::chat::OutboundPacket>, ChatError> =
                        (|| match action {
                            FileAction::Stage(paths) => {
                                if !jobs.can_stage(paths.len()) {
                                    return Err(ChatError::Packet);
                                }
                                for path in paths {
                                    jobs.stage(peer.clone(), path)
                                        .map_err(|_| ChatError::Storage)?;
                                }
                                Ok(Vec::new())
                            }
                            FileAction::Save(id, target) => {
                                match engine.cached_file(&peer, &id, now_ms()) {
                                    Ok((source, info)) => {
                                        jobs.save(peer.clone(), id, source, info, target)
                                            .map_err(|_| ChatError::Storage)?;
                                        Ok(Vec::new())
                                    }
                                    Err(_) => {
                                        let packet = engine.request_file(&peer, &id, now_ms())?;
                                        jobs.set_target(&peer, &id, target);
                                        Ok(vec![packet])
                                    }
                                }
                            }
                            FileAction::Cancel(id) => {
                                jobs.cancel(&peer, &id);
                                engine
                                    .cancel_file(&peer, &id, now_ms())
                                    .map(|packet| vec![packet])
                            }
                            FileAction::Reveal(id) => {
                                if let Some(path) = engine.saved_path(&peer, &id, now_ms())? {
                                    jobs.reveal(peer.clone(), id, path)
                                        .map_err(|_| ChatError::Storage)?;
                                } else {
                                    update(UiEvent::ChooseDestination(peer.clone(), id));
                                }
                                Ok(Vec::new())
                            }
                            FileAction::Clear(id) => {
                                jobs.cancel(&peer, &id);
                                engine.clear_file(&peer, &id, now_ms())
                            }
                        })();
                    match result {
                        Ok(packets) => {
                            if let Some(hub) = &hub {
                                if let Some(transport) = chat_transport.as_mut() {
                                    let _ = transport.connect(hub, &peer).await;
                                }
                                send_chat_packets(
                                    &session_work,
                                    hub,
                                    chat_transport.as_ref(),
                                    packets,
                                )
                                .await;
                            }
                        }
                        Err(_) => update(UiEvent::Toast("chat.file.failed")),
                    }
                    if let Ok(messages) = engine.messages(&peer, now_ms()) {
                        update(UiEvent::ChatMessages(peer, messages, false));
                    }
                }
            });
            continue;
        }
        if matches!(command, Command::ExpireStreams) {
            let Some(ticket) = voice_ticket else {
                continue;
            };
            let work = VoiceWork::new(&intents, ticket, &stop, update.as_ref());
            if !work.is_current() {
                continue;
            }
            let Some(expired) = work.commit_resource(voice_ticket, || {
                stream_drops.take_expired(std::time::Instant::now())
            }) else {
                continue;
            };
            for peer in expired {
                if voice.as_ref().is_some_and(|voice| voice.has_peer(&peer)) {
                    let lifetime = video_jobs
                        .get(&peer)
                        .map(|receiver| receiver.lifetime().clone());
                    let Some(receiver) = work.take_video(
                        &mut video_jobs,
                        voice_ticket,
                        lifetime.as_ref(),
                        &peer,
                        |_| {
                            paused_video.insert(peer.clone());
                            true
                        },
                    ) else {
                        continue;
                    };
                    if let Some(mut receiver) = receiver {
                        receiver.stop();
                        let replaced = work.install_video(
                            &mut video_jobs,
                            voice_ticket,
                            lifetime.as_ref().unwrap(),
                            (peer.clone(), receiver),
                            || {},
                        );
                        drop(replaced);
                    }
                    work.publish(UiEvent::RemoveVideo(peer.clone()));
                    work.publish(UiEvent::Hub(HubEvent {
                        target: "UserStoppedStreaming".into(),
                        arguments: vec![json!(peer)],
                    }));
                }
            }
            continue;
        }
        if matches!(command, Command::Logout) {
            let clearing = session_io.clear(command_ticket);
            let logout_work = SessionWork::new(&intents, command_ticket, &stop, update.as_ref());
            let reset = run_session!(
                logout_work,
                reset_audio_session(&logout_work, &audio_preferences, &audio_runtime)
            );
            let Some(Some(reset)) = reset else {
                continue;
            };
            let Some((old_connection, stop_stream, end_call, disposed)) =
                logout_work.commit(|| {
                    let end_call = abandoned_call
                        || voice
                            .as_ref()
                            .is_some_and(|session| session.call_peer.is_some());
                    let stop_stream =
                        pending_stop_stream || voice.as_ref().is_some_and(VoiceSession::is_sharing);
                    abandoned_call = false;
                    pending_stop_stream = false;
                    video_generation.fetch_add(1, Ordering::AcqRel);
                    chat_failed = false;
                    call_connected = false;
                    retry_at = None;
                    session_ready = false;
                    (
                        hub.take(),
                        stop_stream,
                        end_call,
                        (
                            pending_join.take(),
                            std::mem::take(&mut video_jobs),
                            std::mem::take(&mut paused_video),
                            std::mem::take(&mut stream_drops),
                            events.take(),
                            current_user.take(),
                            file_io.retire_session(
                                file_jobs.take(),
                                chat.take(),
                                chat_transport.take(),
                            ),
                            std::mem::take(&mut file_progress),
                            std::mem::take(&mut friends),
                            selected_chat.take(),
                            incoming_call.take(),
                            saved_credentials.take(),
                            voice.take(),
                            media_events.take(),
                        ),
                    )
                })
            else {
                continue;
            };
            drop((
                disposed.0, disposed.1, disposed.2, disposed.3, disposed.4, disposed.5, disposed.6,
                disposed.7,
            ));
            drop((
                disposed.8,
                disposed.9,
                disposed.10,
                disposed.11,
                disposed.12,
                disposed.13,
            ));
            logout_work.publish(UiEvent::VoiceEnded);
            logout_work.publish(UiEvent::SignedOut);
            if let Some(connection) = old_connection {
                close_logout_connection(connection, &stop, stop_stream, end_call).await;
            }
            if !reset {
                logout_work.publish(UiEvent::Toast("validation.sessionSaveError"));
            }
            if matches!(run_session!(logout_work, clearing), Some(Err(_))) {
                logout_work.publish(UiEvent::Toast("validation.sessionSaveError"));
            }
            if matches!(run_session!(logout_work, file_io.finish()), Some(Err(_))) {
                logout_work.publish(UiEvent::Toast("chat.file.failed"));
            }
            continue;
        }
        if matches!(command, Command::ResumeCall) {
            let Some(ticket) = voice_ticket else {
                continue;
            };
            let work = VoiceWork::new(&intents, ticket, &stop, update.as_ref());
            if !work.is_current() {
                continue;
            }
            let session_connected =
                session_ready && hub.as_ref().is_some_and(HubConnection::is_connected);
            let Some(action) = work.commit_resource(voice_ticket, || {
                call_recovery.poll(std::time::Instant::now(), session_connected)
            }) else {
                continue;
            };
            match action {
                RecoveryAction::Wait => continue,
                RecoveryAction::End => command = Command::Leave,
                RecoveryAction::Resume {
                    session,
                    peer,
                    timeout,
                } => {
                    let Some(connection) = hub.as_ref() else {
                        continue;
                    };
                    let result = work
                        .run(zabor_native::call_recovery::resume(
                            connection, &session, timeout,
                        ))
                        .await;
                    let Some(result) = result else {
                        continue;
                    };
                    match result {
                        Ok(ResumeResult::Ended) => {
                            if work
                                .commit_resource(voice_ticket, || command = Command::Leave)
                                .is_none()
                            {
                                continue;
                            }
                        }
                        Ok(ResumeResult::Resumed { peer: user })
                            if user.id == peer
                                && call_recovery.can_restore(
                                    &session,
                                    &peer,
                                    std::time::Instant::now(),
                                ) =>
                        {
                            if let Some(voice) = voice.as_mut() {
                                let duration = call_recovery
                                    .remaining(std::time::Instant::now())
                                    .min(Duration::from_secs(5));
                                let future = work.run(tokio::time::timeout(
                                    duration,
                                    voice.resume_call(connection, &peer, chat_ice.clone()),
                                ));
                                let result = tokio::select! { _ = stop.cancelled() => break, result = future => result };
                                let Some(result) = result else {
                                    continue;
                                };
                                let mut retired_video = None;
                                let Some(restored) = work.commit_resource(voice_ticket, || {
                                    if matches!(result, Ok(Ok(())))
                                        && call_recovery.restored(
                                            &session,
                                            &peer,
                                            std::time::Instant::now(),
                                        )
                                    {
                                        retired_video = video_jobs.remove(&peer);
                                        paused_video.remove(&peer);
                                        true
                                    } else {
                                        call_recovery.retry(std::time::Instant::now());
                                        false
                                    }
                                }) else {
                                    continue;
                                };
                                drop(retired_video);
                                if restored {
                                    stream_drops.cancel(&peer);
                                    work.publish(UiEvent::RemoveVideo(peer));
                                    work.publish(UiEvent::Voice(String::new(), vec![*user]));
                                    if matches!(
                                        work.run(voice.restore_share(connection)).await,
                                        Some(Err(_))
                                    ) {
                                        work.publish(UiEvent::Toast("toasts.streamStartFailed"));
                                    }
                                }
                            }
                            continue;
                        }
                        _ => {
                            work.commit_resource(voice_ticket, || {
                                call_recovery.retry(std::time::Instant::now())
                            });
                            continue;
                        }
                    }
                }
            }
        }
        if matches!(command, Command::Leave) {
            voice_leave_state!()
                .leave(
                    &intents,
                    command_ticket,
                    &stop,
                    hub.as_ref(),
                    update.as_ref(),
                )
                .await;
            continue;
        }
        if matches!(
            command,
            Command::ChatOpen(_) | Command::ChatSend(_, _) | Command::ChatDelete(_, _)
        ) {
            let session_work = SessionWork::new(&intents, command_ticket, &stop, update.as_ref());
            run_session!(session_work, async {
                let update = |event| session_work.publish(event);
                let Some(engine) = chat.as_mut() else {
                    update(UiEvent::Toast("chat.storageFailed"));
                    return;
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
                        let packets = engine.remove(&peer, &id, now_ms());
                        (peer, packets, false)
                    }
                    _ => unreachable!(),
                };
                match result {
                    Ok(packets) => {
                        if let Some(hub) = hub.as_ref().filter(|hub| hub.is_connected()) {
                            send_chat_packets(&session_work, hub, chat_transport.as_ref(), packets)
                                .await;
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
            });
            continue;
        }
        if matches!(command, Command::PruneChat) {
            let session_work = SessionWork::new(&intents, command_ticket, &stop, update.as_ref());
            run_session!(session_work, async {
                let update = |event| session_work.publish(event);
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
            });
            continue;
        }
        if hub.as_ref().is_none_or(|hub| !hub.is_connected()) {
            audio_preferences.disconnect();
            let token = match signing.as_ref().map(Attestation::token).transpose() {
                Ok(token) => token,
                Err(_) => {
                    update(UiEvent::Error(AuthError::ClientRejected));
                    break;
                }
            };
            let connection_work =
                SessionWork::new(&intents, command_ticket, &stop, update.as_ref());
            let resume_budget = call_recovery
                .session_for_login(std::time::Instant::now())
                .map(|_| call_recovery.remaining(std::time::Instant::now()));
            let connecting = async {
                let future = session_io
                    .hub_io
                    .connect(&server, token.as_ref().map(|token| token.as_str()));
                match resume_budget {
                    Some(budget) => tokio::time::timeout(budget, future)
                        .await
                        .map_err(|_| zabor_native::hub::HubError::Timeout)?,
                    None => future.await,
                }
            };
            let connection = run_session!(connection_work, connecting);
            let Some(connection) = connection else {
                continue;
            };
            match connection {
                Ok((connection, receiver)) => {
                    if connection_work
                        .commit(|| {
                            hub = Some(connection);
                            events = Some(receiver);
                        })
                        .is_none()
                    {
                        continue;
                    }
                    connection_work.publish(UiEvent::Connected);
                }
                Err(error) => {
                    let error = AuthError::from(error);
                    connection_work.commit(|| {
                        if saved_credentials.is_some() && error != AuthError::ClientRejected {
                            retry_attempt = retry_attempt.saturating_add(1);
                            retry_at = Some(
                                tokio::time::Instant::now()
                                    + Duration::from_secs((2_u64 << retry_attempt.min(4)).min(30)),
                            );
                        }
                    });
                    connection_work.publish(UiEvent::Error(error));
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
            let resume_session = call_recovery
                .session_for_login(std::time::Instant::now())
                .map(str::to_owned);
            let auth_work = SessionWork::new(&intents, command_ticket, &stop, update.as_ref());
            let credentials = saved_credentials.as_ref().unwrap().clone();
            let resume_budget = resume_session
                .as_ref()
                .map(|_| call_recovery.remaining(std::time::Instant::now()));
            let resume_work =
                voice_ticket.map(|ticket| VoiceWork::new(&intents, ticket, &stop, update.as_ref()));
            let authenticating = async {
                let future =
                    auth.login_with_resume_if(&credentials, resume_session.as_deref(), || {
                        resume_work.as_ref().is_some_and(VoiceWork::is_current)
                    });
                match resume_budget {
                    Some(budget) => tokio::time::timeout(budget, future)
                        .await
                        .map_err(|_| AuthError::Network)?,
                    None => future.await,
                }
            };
            let outcome = run_session!(auth_work, authenticating);
            let Some(outcome) = outcome else {
                continue;
            };
            match outcome {
                Ok(LoginOutcome::SignedIn(data)) => {
                    let restored = run_session!(
                        auth_work,
                        restore_audio_session(&auth_work, &audio_preferences, &connection, &data)
                    );
                    let Some(Some(restored)) = restored else {
                        continue;
                    };
                    let previous = auth_work.commit(|| {
                        sent_audio_state = None;
                        sent_speaking = false;
                        speaking_release = None;
                        session_ready = true;
                        retry_attempt = 0;
                        retry_at = None;
                        current_user = Some(data.user.clone());
                        chat_ice = data
                            .ice_config
                            .as_ref()
                            .and_then(|value| {
                                serde_json::from_value(value["iceServers"].clone()).ok()
                            })
                            .unwrap_or_default();
                        std::mem::replace(
                            &mut friends,
                            data.friends
                                .iter()
                                .map(|friend| friend.id.clone())
                                .collect(),
                        )
                    });
                    let Some(previous) = previous else {
                        continue;
                    };
                    if run_session!(
                        auth_work,
                        chat_io::reconcile(
                            &auth_work,
                            &chat_io,
                            chat,
                            chat_transport,
                            file_jobs,
                            &previous,
                            &friends
                        )
                    )
                    .is_none()
                    {
                        continue;
                    }
                    if !restored {
                        auth_work.publish(UiEvent::Toast("validation.sessionSaveError"));
                    }
                    auth_work.publish(UiEvent::SignedIn(data));
                    let restore_work = voice_ticket
                        .map(|ticket| VoiceWork::new(&intents, ticket, &stop, update.as_ref()));
                    if let Some(session) = voice.as_ref() {
                        if let Some(channel) = session.channel.clone() {
                            let Some(work) = &restore_work else {
                                continue;
                            };
                            if !work.is_current() {
                                continue;
                            }
                            let Some(mut previous) = work.take_previous_media(
                                &mut voice,
                                &mut media_events,
                                &mut video_jobs,
                                voice_ticket,
                                false,
                                |_| {
                                    video_generation.fetch_add(1, Ordering::AcqRel);
                                    paused_video.clear();
                                    stream_drops.clear();
                                },
                            ) else {
                                continue;
                            };
                            for (peer, receiver) in &mut previous.2 {
                                work.publish(UiEvent::RemoveVideo(peer.clone()));
                                receiver.stop();
                            }
                            let result = work
                                .run(previous.0.as_mut().unwrap().rejoin(&connection))
                                .await;
                            let Some(result) = result else {
                                continue;
                            };
                            match result {
                                Ok(users) => {
                                    let Some(retired) = work.install_previous_media(
                                        &mut voice,
                                        &mut media_events,
                                        voice_ticket,
                                        previous,
                                        || {},
                                    ) else {
                                        continue;
                                    };
                                    drop(retired);
                                    if let Some(work) = &restore_work {
                                        work.publish(UiEvent::Voice(channel, users));
                                        if let Some(session) = voice.as_ref() {
                                            if matches!(
                                                work.run(session.restore_share(&connection)).await,
                                                Some(Err(_))
                                            ) {
                                                work.publish(UiEvent::Toast(
                                                    "toasts.streamStartFailed",
                                                ));
                                            }
                                        }
                                    }
                                }
                                Err(_) => {
                                    drop(previous);
                                    if let Some(work) = &restore_work {
                                        work.publish(UiEvent::VoiceEnded);
                                        work.publish(UiEvent::Toast("toasts.channelJoinFailed"));
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(LoginOutcome::NeedsRegistration)
                | Err(AuthError::InvalidPassword | AuthError::ClientRejected) => {
                    let clearing = session_io.clear(command_ticket);
                    let reset = run_session!(
                        auth_work,
                        reset_audio_session(&auth_work, &audio_preferences, &audio_runtime)
                    );
                    let Some(Some(reset)) = reset else {
                        continue;
                    };
                    let Some(disposed) = auth_work.commit(|| {
                        chat_failed = false;
                        (
                            file_io.retire_session(
                                file_jobs.take(),
                                chat.take(),
                                chat_transport.take(),
                            ),
                            voice.take(),
                            media_events.take(),
                            saved_credentials.take(),
                            current_user.take(),
                            std::mem::take(&mut file_progress),
                            std::mem::take(&mut friends),
                            selected_chat.take(),
                        )
                    }) else {
                        continue;
                    };
                    drop(disposed);
                    auth_work.publish(UiEvent::VoiceEnded);
                    auth_work.publish(UiEvent::SignedOut);
                    if !reset {
                        auth_work.publish(UiEvent::Toast("validation.sessionSaveError"));
                    }
                    if matches!(run_session!(auth_work, clearing), Some(Err(_))) {
                        auth_work.publish(UiEvent::Toast("validation.sessionSaveError"));
                    }
                    if matches!(run_session!(auth_work, file_io.finish()), Some(Err(_))) {
                        auth_work.publish(UiEvent::Toast("chat.file.failed"));
                    }
                    continue;
                }
                Err(error) => {
                    auth_work.commit(|| {
                        let delay = match error {
                            AuthError::Throttled(minutes) => minutes.saturating_mul(60),
                            _ => {
                                retry_attempt = retry_attempt.saturating_add(1);
                                (2_u64 << retry_attempt.min(4)).min(30)
                            }
                        };
                        retry_at = Some(
                            tokio::time::Instant::now() + Duration::from_secs(delay.min(86400)),
                        );
                    });
                    auth_work.publish(UiEvent::Error(error));
                    continue;
                }
            }
        }
        if matches!(command, Command::Reconnect) {
            continue;
        }
        if !intents.is_current(command_ticket, command.scope()) {
            continue;
        }
        if main_session::is_session_command(&command) {
            let work = SessionWork::new(&intents, command_ticket, &stop, update.as_ref());
            if let Command::ProfileAddFriend(query) = &command {
                if friends.contains(&query.id) {
                    work.publish(UiEvent::ProfileFriendResult(query.clone(), false));
                    continue;
                }
            }
            let profile_query = if let Command::UpdateProfile(_, query) = &command {
                Some(query.clone())
            } else {
                None
            };
            let member_channel = match &command {
                Command::MemberAction(request)
                    if matches!(request.operation, member_actions::Operation::Invite { .. }) =>
                {
                    VoiceWork::new(&intents, request.ticket, &stop, update.as_ref())
                        .commit_resource(voice_ticket, || {
                            voice.as_ref().and_then(|voice| voice.channel.clone())
                        })
                        .flatten()
                }
                _ => None,
            };
            let result = run_session!(work, async {
                match command {
                    Command::Achievements(query) => {
                        main_session::achievements(
                            query,
                            &connection,
                            current_user.as_ref(),
                            &intents,
                            &stop,
                        )
                        .await
                    }
                    Command::MemberAction(request) => {
                        let friend = matches!(&request.operation, member_actions::Operation::RemoveFriend(target) if friends.contains(target));
                        let result = match current_user.as_ref() {
                            Some(user) => {
                                member_actions::execute(
                                    &request,
                                    &connection,
                                    user,
                                    friend,
                                    member_channel.as_deref(),
                                    &intents,
                                )
                                .await
                            }
                            None => Err(AuthError::Network),
                        };
                        Ok(MainResult::Event(UiEvent::Members(member_ui::Event {
                            request,
                            result,
                        })))
                    }
                    command => {
                        main_session::compute(command, &auth, &connection, current_user.clone())
                            .await
                    }
                }
            });
            let Some(result) = result else {
                continue;
            };
            match result {
                Ok(MainResult::Authentication(LoginOutcome::NeedsRegistration, _)) => {
                    work.publish(UiEvent::MissingAccount)
                }
                Ok(MainResult::Authentication(LoginOutcome::SignedIn(data), credentials)) => {
                    if profile.is_some() {
                        let prepared = run_session!(
                            work,
                            session_io.prepare(data.user.id.clone(), credentials.clone())
                        );
                        match prepared {
                            Some(Ok(prepared)) => {
                                match run_session!(work, session_io.save(command_ticket, prepared))
                                {
                                    Some(Ok(true)) => {}
                                    Some(Err(_)) => {
                                        work.publish(UiEvent::Toast("validation.sessionSaveError"))
                                    }
                                    Some(Ok(false)) | None => continue,
                                }
                            }
                            Some(Err(_)) => {
                                work.publish(UiEvent::Toast("validation.sessionSaveError"))
                            }
                            None => continue,
                        }
                    }
                    let restored = run_session!(
                        work,
                        restore_audio_session(&work, &audio_preferences, &connection, &data)
                    );
                    let Some(Some(restored)) = restored else {
                        continue;
                    };
                    if work
                        .commit(|| {
                            sent_audio_state = None;
                            sent_speaking = false;
                            speaking_release = None;
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
                            saved_credentials = Some(credentials);
                            session_ready = true;
                            retry_attempt = 0;
                            retry_at = None;
                        })
                        .is_none()
                    {
                        continue;
                    }
                    if !restored {
                        work.publish(UiEvent::Toast("validation.sessionSaveError"));
                    }
                    work.publish(UiEvent::SignedIn(data));
                }
                Ok(MainResult::Event(event)) => {
                    if let UiEvent::App(app_ui::Event::Achievements(result)) = event {
                        let mut job = achievement_io.start(*result, update.clone());
                        let completed = run_session!(work, &mut job.receiver);
                        let joined = match run_session!(work, achievement_io.join(job.id)) {
                            Some(joined) => joined,
                            None => achievement_io.join(job.id).await,
                        };
                        if joined.is_err() {
                            work.publish(UiEvent::Toast("validation.sessionSaveError"));
                            if let Some(Ok(UiEvent::App(app_ui::Event::Achievements(result)))) =
                                completed
                            {
                                if result.storage_failed {
                                    work.publish(UiEvent::App(app_ui::Event::Achievements(result)));
                                }
                            }
                        } else if let Some(Ok(event)) = completed {
                            work.publish(event);
                        }
                    } else {
                        work.publish(event);
                    }
                }
                Ok(MainResult::Profile(result)) => {
                    if !result.query.alive() {
                        continue;
                    }
                    let user = work
                        .commit(|| {
                            let user = current_user
                                .as_mut()
                                .filter(|user| user.id == result.user.id)?;
                            user.display_name = result.user.display_name;
                            user.avatar_base64 = result.user.avatar_base64;
                            user.avatar_color = result.user.avatar_color;
                            user.about_me = result.user.about_me;
                            Some(user.clone())
                        })
                        .flatten();
                    if let Some(user) = user {
                        work.publish(UiEvent::ProfileSaved(user, result.query));
                    }
                }
                Err(error) => work.publish(if let Some(query) = profile_query {
                    UiEvent::ProfileError(query, error)
                } else {
                    UiEvent::Error(error)
                }),
            }
            continue;
        }
        if matches!(command, Command::Audio) {
            let work = SessionWork::new(&intents, command_ticket, &stop, update.as_ref());
            let (muted, deafened) = audio_controls.user_state();
            if work
                .commit(|| {
                    if let Some(user) = current_user.as_mut() {
                        user.is_muted = muted;
                        user.is_deafened = deafened;
                    }
                })
                .is_none()
            {
                continue;
            }
            if let (Some(session), Some(ticket)) = (voice.as_mut(), voice_ticket) {
                let voice_work = VoiceWork::new(&intents, ticket, &stop, update.as_ref());
                match voice_work
                    .run(session.mute(&connection, muted, deafened))
                    .await
                {
                    Some(Ok(())) => {
                        if voice_work.is_current() {
                            sent_audio_state = Some((muted, deafened));
                        }
                    }
                    Some(Err(_)) => {
                        work.publish(UiEvent::Error(AuthError::Network));
                        continue;
                    }
                    None => {}
                }
            }
            work.publish(UiEvent::Connected);
            continue;
        }
        let operation_scope = command.scope();
        let starting_call = matches!(command, Command::Call(_) | Command::AnswerCall(_, true));
        let call_failure = match &command {
            Command::Call(peer) => Some((
                current_user
                    .as_ref()
                    .map(|user| user.id.clone())
                    .unwrap_or_default(),
                peer.clone(),
            )),
            _ => None,
        };
        let voice_work = VoiceWork::new(&intents, command_ticket, &stop, update.as_ref());
        let previous_voice_work = VoiceWork::new(
            &intents,
            voice_ticket.unwrap_or(command_ticket),
            &stop,
            update.as_ref(),
        );
        let operation = async {
            match command {
                Command::Authenticate { .. }
                | Command::Captcha
                | Command::Modal { .. }
                | Command::UpdateProfile(..)
                | Command::LookupProfile(..)
                | Command::ProfileAddFriend(..)
                | Command::Achievements(..)
                | Command::MemberAction(..)
                | Command::Audio => unreachable!(),
                Command::Notification(kind, id, accept) => {
                    if !respond_notification(&connection, kind, &id, accept)
                        .await
                        .map_err(AuthError::from)?
                    {
                        return Err(AuthError::Network);
                    }
                    if kind == NotificationKind::Channel && accept && voice.is_none() {
                        pending_join = Some(id.clone());
                    }
                    Ok(UiEvent::NotificationResolved(kind, id, accept))
                }
                Command::Logout
                | Command::Reconnect
                | Command::ResumeCall
                | Command::Leave
                | Command::ChatOpen(_)
                | Command::ChatSend(_, _)
                | Command::ChatDelete(_, _)
                | Command::PruneChat => unreachable!(),
                Command::File { .. } | Command::MaintainFiles => unreachable!(),
                Command::ExpireStreams => unreachable!(),
                Command::Join(channel) => {
                    if voice
                        .as_ref()
                        .is_some_and(|voice| voice.call_peer.is_some())
                    {
                        let Some(mut previous) = previous_voice_work.take_previous_media(
                            &mut voice,
                            &mut media_events,
                            &mut video_jobs,
                            voice_ticket,
                            false,
                            |_| {
                                call_recovery.clear();
                                abandoned_call = true;
                                video_generation.fetch_add(1, Ordering::AcqRel);
                                paused_video.clear();
                                stream_drops.clear();
                            },
                        ) else {
                            return Err(AuthError::Network);
                        };
                        if let Some(session) = previous.0.as_mut() {
                            session.leave(&connection).await;
                            if previous_voice_work
                                .commit(|| abandoned_call = false)
                                .is_none()
                            {
                                return Err(AuthError::Network);
                            }
                        }
                        drop(previous);
                        update(UiEvent::VoiceEnded.guarded(command_ticket, IntentScope::Voice));
                    }
                    let Some(user) = current_user.as_ref() else {
                        return Err(AuthError::Network);
                    };
                    if voice.is_none() {
                        let (session, receiver) = VoiceSession::new(
                            &connection,
                            user,
                            audio_preferences.relay_only(),
                            audio_controls.clone(),
                            audio_runtime.clone(),
                        )
                        .await
                        .map_err(|_| AuthError::Network)?;
                        let Some(replaced) = voice_work.install_voice(
                            &mut voice,
                            &mut media_events,
                            &mut voice_ticket,
                            (session, receiver),
                            || {},
                        ) else {
                            return Err(AuthError::Network);
                        };
                        drop(replaced);
                    }
                    let Some(ticket) = voice_ticket else {
                        return Err(AuthError::Network);
                    };
                    let join_work = VoiceWork::new(&intents, ticket, &stop, update.as_ref());
                    if join_work
                        .commit_resource(voice_ticket, || {
                            sent_audio_state = None;
                            sent_speaking = false;
                            speaking_release = None;
                        })
                        .is_none()
                    {
                        return Err(AuthError::Network);
                    }
                    let Some(joined) = join_work
                        .run(voice.as_mut().unwrap().join(&connection, &channel))
                        .await
                    else {
                        return Err(AuthError::Network);
                    };
                    match joined {
                        Ok(users) => {
                            let Some(retired) = join_work.retire_videos(
                                &mut video_jobs,
                                voice_ticket,
                                |peer, _| !users.iter().any(|user| user.id == peer),
                            ) else {
                                return Err(AuthError::Network);
                            };
                            drop(retired);
                            Ok(UiEvent::Voice(channel, users))
                        }
                        Err(error) => {
                            if voice.as_ref().is_some_and(|voice| voice.channel.is_none()) {
                                let Some(disposed) = join_work.take_voice(
                                    &mut voice,
                                    &mut media_events,
                                    voice_ticket,
                                    |_| {},
                                ) else {
                                    return Err(AuthError::Network);
                                };
                                drop(disposed);
                            }
                            Ok(UiEvent::Toast(match error {
                                MediaError::Microphone => "toasts.micNotFound",
                                MediaError::ChannelFull => "modals.channelFull.title",
                                _ => "toasts.channelJoinFailed",
                            }))
                        }
                    }
                }
                Command::Call(peer) => {
                    let Some(mut previous) = previous_voice_work.take_call_media(
                        &mut voice,
                        &mut media_events,
                        &mut video_jobs,
                        voice_ticket,
                        friends.contains(&peer),
                        |_| {
                            video_generation.fetch_add(1, Ordering::AcqRel);
                            paused_video.clear();
                            stream_drops.clear();
                        },
                    ) else {
                        return Err(AuthError::Network);
                    };
                    if let Some(session) = previous.0.as_mut() {
                        if session.call_peer.is_some() {
                            let _ = connection.send("EndCall", Vec::new()).await;
                        } else {
                            session.leave(&connection).await;
                        }
                    }
                    drop(previous);
                    if previous_voice_work
                        .commit(|| {
                            call_connected = false;
                            sent_speaking = false;
                            speaking_release = None;
                            sent_audio_state = None;
                        })
                        .is_none()
                    {
                        return Err(AuthError::Network);
                    }
                    let user = current_user.as_ref().ok_or(AuthError::Network)?;
                    let (mut session, receiver) = VoiceSession::new(
                        &connection,
                        user,
                        audio_preferences.relay_only(),
                        audio_controls.clone(),
                        audio_runtime.clone(),
                    )
                    .await
                    .map_err(|_| AuthError::Network)?;
                    session
                        .prepare_call(&peer)
                        .await
                        .map_err(|_| AuthError::Network)?;
                    let accepted = connection
                        .invoke("StartCall", vec![json!(peer)], Duration::from_secs(20))
                        .await
                        .map_err(AuthError::from)?;
                    if accepted != true {
                        return Ok(UiEvent::VoiceEnded);
                    }
                    let Some(replaced) = voice_work.install_voice(
                        &mut voice,
                        &mut media_events,
                        &mut voice_ticket,
                        (session, receiver),
                        || call_recovery.begin(&peer),
                    ) else {
                        return Err(AuthError::Network);
                    };
                    drop(replaced);
                    Ok(UiEvent::Calling(peer))
                }
                Command::ListScreens(query, own_window, kind) => {
                    let inventory_update = update.clone();
                    let inventory_query = query.clone();
                    let preview_update = update.clone();
                    let preview_query = query.clone();
                    match ScreenCapture::sources_with_audio_runtime(
                        &audio_runtime,
                        own_window,
                        kind,
                        query.clone(),
                        move |sources| {
                            inventory_update(
                                UiEvent::Screens(inventory_query, Some(sources))
                                    .guarded(command_ticket, IntentScope::Voice),
                            );
                        },
                        move |source, image| {
                            preview_update(
                                UiEvent::SourcePreview(preview_query.clone(), source, image)
                                    .guarded(command_ticket, IntentScope::Voice),
                            );
                        },
                    )
                    .await
                    {
                        Ok(sources) => {
                            screen_sources = sources;
                            Ok(UiEvent::Connected)
                        }
                        Err(_) => Ok(UiEvent::Screens(query, None)),
                    }
                }
                Command::ListCameras(query) => {
                    let inventory_update = update.clone();
                    let inventory_query = query.clone();
                    let preview_update = update.clone();
                    let preview_query = query.clone();
                    match CameraCapture::picker_with_audio_runtime(
                        &audio_runtime,
                        query.clone(),
                        move |sources| {
                            inventory_update(
                                UiEvent::Cameras(inventory_query, Some(sources))
                                    .guarded(command_ticket, IntentScope::Voice),
                            );
                        },
                        move |source, image| {
                            preview_update(
                                UiEvent::SourcePreview(preview_query.clone(), source, image)
                                    .guarded(command_ticket, IntentScope::Voice),
                            );
                        },
                    )
                    .await
                    {
                        Ok(sources) => {
                            camera_sources = sources;
                            Ok(UiEvent::Connected)
                        }
                        Err(_) => Ok(UiEvent::Cameras(query, None)),
                    }
                }
                Command::StartShare(id, quality, include_audio, allow_control) => {
                    if let Some(device) = id.strip_prefix("camera:") {
                        if quality != "camera"
                            || device.is_empty()
                            || !camera_sources.iter().any(|source| source.id == device)
                        {
                            return Err(AuthError::Network);
                        }
                        let Some(mut resources) = voice_work.take_voice(
                            &mut voice,
                            &mut media_events,
                            voice_ticket,
                            |_| {},
                        ) else {
                            return Err(AuthError::Network);
                        };
                        let result = if let Some(session) = resources.0.as_mut() {
                            session
                                .start_camera_share(&connection, device.to_owned())
                                .await
                                .map_err(|_| AuthError::Network)
                        } else {
                            Err(AuthError::Network)
                        };
                        let Some(replaced) = voice_work.install_previous_media(
                            &mut voice,
                            &mut media_events,
                            voice_ticket,
                            (resources.0, resources.1, BTreeMap::new()),
                            || {},
                        ) else {
                            return Err(AuthError::Network);
                        };
                        drop(replaced);
                        result?;
                        return Ok(UiEvent::Sharing(true));
                    }
                    let source = screen_sources
                        .iter()
                        .find(|source| source.key() == id)
                        .cloned()
                        .ok_or(AuthError::Network)?;
                    let Some(mut resources) =
                        voice_work.take_voice(&mut voice, &mut media_events, voice_ticket, |_| {})
                    else {
                        return Err(AuthError::Network);
                    };
                    let result = if let Some(session) = resources.0.as_mut() {
                        session
                            .start_share_with_control(
                                &connection,
                                source,
                                &quality,
                                include_audio,
                                allow_control,
                            )
                            .await
                            .map_err(|_| AuthError::Network)
                    } else {
                        Err(AuthError::Network)
                    };
                    let Some(replaced) = voice_work.install_previous_media(
                        &mut voice,
                        &mut media_events,
                        voice_ticket,
                        (resources.0, resources.1, BTreeMap::new()),
                        || {},
                    ) else {
                        return Err(AuthError::Network);
                    };
                    drop(replaced);
                    result?;
                    Ok(UiEvent::Sharing(true))
                }
                Command::FocusStreamAudio(peer) => {
                    if let Some(session) = voice.as_mut() {
                        let _ = session
                            .set_stream_focus(&connection, (!peer.is_empty()).then_some(peer))
                            .await;
                    }
                    Ok(UiEvent::Connected)
                }
                Command::ToggleControl(target) => {
                    let session = voice
                        .as_mut()
                        .filter(|session| session.control_target_current(&target))
                        .ok_or(AuthError::Network)?;
                    session.bind_control_intents(
                        intents.clone(),
                        target.input_ticket().ok_or(AuthError::Network)?,
                    );
                    session
                        .toggle_remote_control_target(&connection, target.clone())
                        .await
                        .map_err(|_| AuthError::Network)?;
                    Ok(UiEvent::RemoteControl(Box::new(
                        rc_desktop::Update::from_session(session, Some(target)),
                    )))
                }
                Command::ControlInput(target, packet) => {
                    let session = voice
                        .as_mut()
                        .filter(|session| session.control_target_current(&target))
                        .ok_or(AuthError::Network)?;
                    match session.send_control_target_input(&target, packet) {
                        Ok(true) => Ok(UiEvent::ControlActivity(target)),
                        Ok(false) => Ok(UiEvent::Connected),
                        Err(_) => Ok(UiEvent::RemoteControl(Box::new(
                            rc_desktop::Update::from_session(session, Some(target)),
                        ))),
                    }
                }
                Command::ReleaseControlRequest(target) => {
                    if let Some(session) = voice.as_mut() {
                        session.release_remote_control_target(&target);
                        Ok(UiEvent::RemoteControl(Box::new(
                            rc_desktop::Update::from_session(session, Some(target)),
                        )))
                    } else {
                        Ok(UiEvent::Connected)
                    }
                }
                Command::RevokeControlTarget(target) => {
                    if let Some(session) = voice.as_mut() {
                        let original = session.control_viewer_target();
                        session.revoke_remote_control_target(&target);
                        Ok(UiEvent::RemoteControl(Box::new(
                            rc_desktop::Update::from_session(session, original),
                        )))
                    } else {
                        Ok(UiEvent::Connected)
                    }
                }
                #[cfg(test)]
                Command::ReleaseControl | Command::RevokeControl => Ok(UiEvent::Connected),
                Command::MaintainStreams => {
                    let original = voice.as_ref().and_then(VoiceSession::control_viewer_target);
                    if let Some(session) = voice.as_mut() {
                        session.maintain_streams(&connection).await;
                        if let Some(target) = session.take_control_activity() {
                            voice_work.publish(UiEvent::ControlActivity(target));
                        }
                    }
                    if let Some(retired) =
                        voice_work.retire_videos(&mut video_jobs, voice_ticket, |_, receiver| {
                            !receiver.is_alive()
                        })
                    {
                        for (peer, receiver) in retired {
                            if !voice
                                .as_ref()
                                .is_some_and(|voice| voice.has_stream_snapshot(&peer))
                            {
                                voice_work.publish(UiEvent::RemoveVideo(peer));
                            }
                            drop(receiver);
                        }
                    }
                    if let Some(session) = voice.as_ref() {
                        Ok(UiEvent::RemoteControl(Box::new(
                            rc_desktop::Update::from_session(session, original),
                        )))
                    } else {
                        Ok(UiEvent::Connected)
                    }
                }
                Command::StopShare => {
                    if let Some(session) = voice.as_mut() {
                        session.stop_share(&connection).await;
                    }
                    Ok(UiEvent::Sharing(false))
                }
                Command::CheckShare => {
                    if let Some(session) = voice.as_mut() {
                        session.check_share_audio();
                    }
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
                    let Some(mut previous) = previous_voice_work.take_previous_media(
                        &mut voice,
                        &mut media_events,
                        &mut video_jobs,
                        voice_ticket,
                        true,
                        |_| {
                            video_generation.fetch_add(1, Ordering::AcqRel);
                            paused_video.clear();
                            stream_drops.clear();
                        },
                    ) else {
                        return Err(AuthError::Network);
                    };
                    for receiver in previous.2.values_mut() {
                        receiver.stop();
                    }
                    if let Some(session) = previous.0.as_mut() {
                        session.leave(&connection).await;
                    }
                    drop(previous);
                    if previous_voice_work
                        .commit(|| {
                            sent_speaking = false;
                            speaking_release = None;
                            sent_audio_state = None;
                        })
                        .is_none()
                    {
                        return Err(AuthError::Network);
                    }
                    let user = current_user.as_ref().ok_or(AuthError::Network)?;
                    let (mut session, receiver) = VoiceSession::new(
                        &connection,
                        user,
                        audio_preferences.relay_only(),
                        audio_controls.clone(),
                        audio_runtime.clone(),
                    )
                    .await
                    .map_err(|_| AuthError::Network)?;
                    session
                        .prepare_call(&peer)
                        .await
                        .map_err(|_| AuthError::Network)?;
                    let accepted = connection
                        .invoke("AcceptCall", vec![json!(peer)], Duration::from_secs(20))
                        .await
                        .map_err(AuthError::from)?;
                    if accepted == false {
                        return Ok(UiEvent::VoiceEnded);
                    }
                    let Some(replaced) = voice_work.install_voice(
                        &mut voice,
                        &mut media_events,
                        &mut voice_ticket,
                        (session, receiver),
                        || {
                            call_recovery.begin(&peer);
                            call_recovery.connected(&peer);
                            call_connected = true;
                        },
                    ) else {
                        return Err(AuthError::Network);
                    };
                    drop(replaced);
                    Ok(UiEvent::Calling(peer))
                }
                Command::CheckAudio => Ok(UiEvent::Connected),
            }
        };
        let result = tokio::select! { _ = stop.cancelled() => break, result = intents.run(command_ticket, operation_scope, operation) => result };
        let Some(result) = result else {
            abandoned_call |= starting_call;
            continue;
        };
        if result.is_err() || matches!(&result, Ok(UiEvent::VoiceEnded)) {
            if let Some((owner, peer)) = call_failure {
                update(
                    UiEvent::App(app_ui::Event::CallFailed { owner, peer })
                        .guarded(command_ticket, operation_scope),
                );
            }
        }
        update(
            result
                .unwrap_or_else(UiEvent::Error)
                .guarded(command_ticket, operation_scope),
        );
    }
    let retired = leave_work::take_ending_media(
        &mut voice,
        &mut media_events,
        &mut video_jobs,
        &mut voice_ticket,
    );
    if let Some(hub) = hub {
        hub.close();
    }
    video_generation.fetch_add(1, Ordering::AcqRel);
    drop(retired);
}

fn apply_mic_test(ui: &NativeApp, snapshot: MicTestSnapshot) {
    ui.set_mic_test(MicTestView {
        stage: match snapshot.stage {
            MicTestStage::Idle => "idle",
            MicTestStage::Arming => "arming",
            MicTestStage::Recording => "recording",
            MicTestStage::Ready => "ready",
        }
        .into(),
        issue: match snapshot.issue {
            Some(MicTestIssue::Silent) => "silent",
            Some(MicTestIssue::Failed) => "failed",
            None => "",
        }
        .into(),
        playing: snapshot.playing,
        duration: snapshot.duration_seconds as f32,
        position: snapshot.position_seconds as f32,
    });
    ui.set_mic_db(snapshot.meter_db);
}

fn synchronize_mic_test(
    ui: &NativeApp,
    test: &mut MicTestUi<NativeAudio>,
    runtime: &tokio::runtime::Handle,
) {
    test.synchronize(
        ui.get_user_id().as_str(),
        ui.get_authenticated() && ui.get_modal() == 3,
        ui.get_settings_tab() == 1,
        ui.get_audio_settings().noise_suppression,
        runtime,
    );
    apply_mic_test(ui, test.snapshot(std::time::Instant::now()));
}

fn apply_audio_settings(ui: &NativeApp, settings: &AudioSettings) {
    ui.set_audio_settings(AudioSettingsView {
        input_volume: settings.input_volume,
        output_volume: settings.output_volume,
        selected_input: settings.selected_input.clone().into(),
        selected_output: settings.selected_output.clone().into(),
        noise_suppression: settings.noise_suppression,
        threshold_mode: match settings.mic_threshold_mode {
            zabor_native::audio_settings::ThresholdMode::Auto => "auto",
            zabor_native::audio_settings::ThresholdMode::Manual => "manual",
        }
        .into(),
        manual_threshold: settings.manual_threshold_value,
        noise_model: match settings.smart_noise_model {
            zabor_native::audio_settings::NoiseModel::Rnnoise => "rnnoise",
            zabor_native::audio_settings::NoiseModel::Deepfilter => "deepfilter",
        }
        .into(),
        speech_analyzer: settings.speech_analyzer,
        echo_cancellation: settings.echo_cancellation,
        suppression_strength: settings.suppression_strength,
        ultra_low_latency: settings.ultra_low_latency,
        relay_only: settings.relay_only,
    });
    refresh_audio_names(ui);
}

fn apply_audio_devices(
    ui: &NativeApp,
    devices: &[zabor_native::audio_device::AudioDevice],
    settings: &AudioSettings,
) {
    let rows = |input, selected: &str| {
        let mut rows = vec![AudioDeviceItem {
            id: "default".into(),
            name: "".into(),
            fallback: "settings.audio.default".into(),
        }];
        let devices = devices
            .iter()
            .filter(|device| device.input == input)
            .take(256)
            .collect::<Vec<_>>();
        if selected != "default" && !devices.iter().any(|device| device.id == selected) {
            rows.push(AudioDeviceItem {
                id: selected.into(),
                name: "".into(),
                fallback: "common.loading".into(),
            });
        }
        rows.extend(devices.into_iter().map(|device| {
            AudioDeviceItem {
                id: device.id.clone().into(),
                name: device.name.clone().into(),
                fallback: if input {
                    "settings.audio.micFallback"
                } else {
                    "settings.audio.speakerFallback"
                }
                .into(),
            }
        }));
        Rc::new(VecModel::from(rows)).into()
    };
    ui.set_input_devices(rows(true, &settings.selected_input));
    ui.set_output_devices(rows(false, &settings.selected_output));
    refresh_audio_names(ui);
}

fn refresh_audio_names(ui: &NativeApp) {
    let Ok(strings) = Strings::new(ui.get_language().as_str()) else {
        return;
    };
    let label = |model: ModelRc<AudioDeviceItem>, selected: &str| {
        model
            .iter()
            .find(|row| row.id == selected)
            .map(|row| {
                if row.name.is_empty() {
                    strings.text(row.fallback.as_str()).into()
                } else {
                    row.name
                }
            })
            .unwrap_or_else(|| strings.text("common.loading").into())
    };
    let settings = ui.get_audio_settings();
    ui.set_input_device_name(label(
        ui.get_input_devices(),
        settings.selected_input.as_str(),
    ));
    ui.set_output_device_name(label(
        ui.get_output_devices(),
        settings.selected_output.as_str(),
    ));
}

fn reconcile_volume_modal(ui: &NativeApp) {
    if ui.get_modal() == 8
        && (!ui.get_authenticated()
            || !ui.get_voice_active()
            || !ui
                .get_voice_users()
                .iter()
                .any(|user| user.id == ui.get_volume_peer()))
    {
        ui.set_modal(0);
        ui.set_volume_peer("".into());
        ui.set_volume_name("".into());
        ui.set_peer_volume(100.0);
    }
}

async fn reset_audio_session<D: zabor_native::audio_runtime::SharedAudioDevice>(
    work: &SessionWork<'_>,
    preferences: &AudioPreferences,
    runtime: &SharedAudio<D>,
) -> Option<bool> {
    loop {
        if !work.is_current() {
            return None;
        }
        if let Some(result) = try_reset_audio_session(work, preferences, runtime) {
            return result;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

fn try_reset_audio_session<D: zabor_native::audio_runtime::SharedAudioDevice>(
    work: &SessionWork<'_>,
    preferences: &AudioPreferences,
    runtime: &SharedAudio<D>,
) -> Option<Option<bool>> {
    let mut logout = match preferences.try_prepare_logout() {
        Ok(None) => return None,
        Ok(logout) => logout,
        Err(_) => None,
    };
    let mut reset = match runtime.try_prepare_reset() {
        Ok(None) => return None,
        Ok(reset) => reset,
        Err(_) => None,
    };
    let result = work.commit(|| {
        if let Some(logout) = &mut logout {
            logout.apply();
        }
        if let Some(reset) = &mut reset {
            reset.apply();
        }
        logout.is_some()
            && reset
                .as_ref()
                .is_some_and(|reset| reset.resources_complete())
    });
    if let Some(reset) = &mut reset {
        reset.release_resources();
    }
    drop(logout);
    drop(reset);
    Some(result)
}

async fn restore_audio_session(
    work: &SessionWork<'_>,
    preferences: &AudioPreferences,
    hub: &HubConnection,
    data: &Bootstrap,
) -> Option<bool> {
    loop {
        if !work.is_current() {
            return None;
        }
        if let Some(result) = try_restore_audio_session(work, preferences, hub, data) {
            return result;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

fn try_restore_audio_session(
    work: &SessionWork<'_>,
    preferences: &AudioPreferences,
    hub: &HubConnection,
    data: &Bootstrap,
) -> Option<Option<bool>> {
    let mut prepared = match preferences.try_prepare_session(
        &data.user.id,
        hub.clone(),
        data.audio_settings.as_ref(),
    ) {
        Ok(None) => return None,
        Ok(prepared) => prepared,
        Err(_) => None,
    };
    Some(work.commit(|| {
        if let Some(prepared) = &mut prepared {
            prepared.apply();
        }
        preferences
            .controls
            .set_user_state(data.user.is_muted, data.user.is_deafened);
        preferences
            .controls
            .set_server_state(data.user.is_server_muted, data.user.is_server_deafened);
        prepared.is_some()
    }))
}

#[cfg(test)]
fn restore_audio(preferences: &AudioPreferences, hub: &HubConnection, data: &Bootstrap) -> bool {
    let restored =
        preferences.begin_session(&data.user.id, hub.clone(), data.audio_settings.as_ref());
    preferences
        .controls
        .set_user_state(data.user.is_muted, data.user.is_deafened);
    preferences
        .controls
        .set_server_state(data.user.is_server_muted, data.user.is_server_deafened);
    restored
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
    check_audio_interactions(ui)?;
    check_mic_test_interactions(ui)?;
    check_stream_picker_interactions(ui)?;
    avatar_ui::check_guards(ui)?;
    avatar_interactions::check(ui)?;
    println!(
        "{}",
        serde_json::json!({"nativeAuthInteractionCheck":"passed","nativeRosterBindingCheck":"passed","nativeAudioInteractionCheck":"passed","nativeMicTestInteractionCheck":"passed","nativeStreamPickerInteractionCheck":"passed","nativePickerThumbnailBindingCheck":"passed","nativeAvatarProfileInteractionCheck":"passed","nativeTextInput":true,"maxLength":25,"passwordVisibility":true,"validationBeforeNetwork":true,"physicalDevices":false})
    );
    Ok(())
}

fn native_window_handle(ui: &NativeApp) -> Option<u64> {
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    ui.window()
        .with_winit_window(|window| match window.window_handle().ok()?.as_raw() {
            RawWindowHandle::Win32(handle) => u64::try_from(handle.hwnd.get()).ok(),
            _ => None,
        })
        .flatten()
}

fn clear_picker_rows(ui: &NativeApp) {
    ui.set_screen_sources(ModelRc::default());
    ui.set_window_sources(ModelRc::default());
    ui.set_camera_sources(ModelRc::default());
    ui.set_selected_screen("".into());
    ui.set_screen_sources_loading(false);
    ui.set_camera_sources_loading(false);
}

fn clear_missing_picker_selection(ui: &NativeApp) {
    let sources = match ui.get_screen_source_kind() {
        0 => ui.get_screen_sources(),
        1 => ui.get_window_sources(),
        _ => ui.get_camera_sources(),
    };
    if !sources.iter().any(|row| row.id == ui.get_selected_screen()) {
        ui.set_selected_screen("".into());
    }
}

fn apply_picker_preview(ui: &NativeApp, source: &str, image: Option<PickerImage>) {
    if let Some(image) = &image {
        if image.width == 0
            || image.height == 0
            || image.width > 320
            || image.height > 180
            || image.rgba.len() != image.width as usize * image.height as usize * 4
        {
            return;
        }
    }
    let (kind, sources) = if source.starts_with("screen:") {
        (0, ui.get_screen_sources())
    } else if source.starts_with("window:") {
        (1, ui.get_window_sources())
    } else if source.starts_with("camera:") {
        (2, ui.get_camera_sources())
    } else {
        return;
    };
    if kind != ui.get_screen_source_kind() || !sources.iter().any(|row| row.id == source) {
        return;
    }
    let thumbnail = image.map_or_else(slint::Image::default, |image| {
        slint::Image::from_rgba8(slint::SharedPixelBuffer::clone_from_slice(
            &image.rgba,
            image.width,
            image.height,
        ))
    });
    let rows = sources
        .iter()
        .map(|mut row| {
            if row.id == source {
                row.thumbnail = thumbnail.clone();
            }
            row
        })
        .collect::<Vec<_>>();
    let rows = Rc::new(VecModel::from(rows)).into();
    match kind {
        0 => ui.set_screen_sources(rows),
        1 => ui.set_window_sources(rows),
        _ => ui.set_camera_sources(rows),
    }
}

fn check_stream_picker_interactions(ui: &NativeApp) -> Result<(), &'static str> {
    use slint::platform::{PointerEventButton, WindowEvent};
    let starts = Rc::new(RefCell::new(Vec::new()));
    let recorded = starts.clone();
    let audio_selections = Rc::new(RefCell::new(Vec::new()));
    let audio_recorded = audio_selections.clone();
    let control_selections = Rc::new(RefCell::new(Vec::new()));
    let control_recorded = control_selections.clone();
    ui.on_start_share(move |source, quality, audio, control| {
        control_recorded.borrow_mut().push(control);
        audio_recorded.borrow_mut().push(audio);
        recorded
            .borrow_mut()
            .push((source.to_string(), quality.to_string()));
    });
    let camera_queries = Rc::new(RefCell::new(0));
    let queries = camera_queries.clone();
    ui.on_list_cameras(move || *queries.borrow_mut() += 1);
    let camera_cancellations = Rc::new(RefCell::new(0));
    let cancellations = camera_cancellations.clone();
    ui.on_cancel_picker_query(move || *cancellations.borrow_mut() += 1);
    ui.on_refresh_screens(|| {});
    ui.set_screen_sources(
        Rc::new(VecModel::from(vec![ScreenItem {
            id: "screen:7".into(),
            title: "screen 1".into(),
            thumbnail: slint::Image::default(),
        }]))
        .into(),
    );
    ui.set_window_sources(
        Rc::new(VecModel::from(vec![ScreenItem {
            id: "window:9".into(),
            title: "editor".into(),
            thumbnail: slint::Image::default(),
        }]))
        .into(),
    );
    ui.set_camera_sources(
        Rc::new(VecModel::from(vec![ScreenItem {
            id: "camera:exact-device-id:with:colons".into(),
            title: "camera 1".into(),
            thumbnail: slint::Image::default(),
        }]))
        .into(),
    );
    ui.set_camera_sources_loading(false);
    ui.set_error("".into());
    ui.set_busy(false);
    ui.set_voice_active(true);
    ui.set_screen_source_kind(0);
    ui.set_screen_quality("low".into());
    ui.set_screen_include_audio(true);
    ui.set_screen_allow_control(false);
    ui.set_selected_screen("".into());
    ui.set_screen_sources_loading(false);
    ui.set_modal(5);
    let click = |x, y| {
        let y = match y as i32 {
            254 => 194.0,
            318 => 258.0,
            450 => 368.0,
            475 => 535.0,
            582 => 642.0,
            _ => y,
        };
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
    if native_window_handle(ui).is_none() {
        return Err("Native picker own-window identity is unavailable");
    }
    let image = PickerImage::from_bgra(&[19, 37, 71, 255].repeat(8), 16, 4, 2)
        .map_err(|_| "Synthetic picker image conversion failed")?;
    apply_picker_preview(ui, "screen:7", Some(image.clone()));
    let thumbnail_size = ui
        .get_screen_sources()
        .row_data(0)
        .unwrap()
        .thumbnail
        .size();
    if (thumbnail_size.width, thumbnail_size.height) != (4, 2) {
        return Err("Picker thumbnail binding lost the source pixels");
    }
    apply_picker_preview(ui, "screen:unknown", Some(image.clone()));
    apply_picker_preview(
        ui,
        "camera:exact-device-id:with:colons",
        Some(image.clone()),
    );
    if ui.get_screen_sources().row_count() != 1
        || ui
            .get_camera_sources()
            .row_data(0)
            .unwrap()
            .thumbnail
            .size()
            .width
            != 0
    {
        return Err("Picker thumbnail entered an unknown source or the wrong tab");
    }
    apply_picker_preview(ui, "screen:7", None);
    if ui
        .get_screen_sources()
        .row_data(0)
        .unwrap()
        .thumbnail
        .size()
        .width
        != 0
    {
        return Err("Failed picker preview retained stale pixels");
    }
    apply_picker_preview(ui, "screen:7", Some(image));
    let first = ui.get_screen_sources().row_data(0).unwrap();
    ui.set_screen_sources(
        Rc::new(VecModel::from(vec![
            first.clone(),
            ScreenItem {
                id: "screen:8".into(),
                title: "screen 2".into(),
                thumbnail: slint::Image::default(),
            },
            ScreenItem {
                id: "screen:10".into(),
                title: "screen 3".into(),
                thumbnail: slint::Image::default(),
            },
        ]))
        .into(),
    );
    click(670.0, 318.0);
    if ui.get_selected_screen() != "screen:8" {
        return Err("Picker second-column source selection failed");
    }
    ui.set_selected_screen("".into());
    click(450.0, 430.0);
    if ui.get_selected_screen() != "screen:10" {
        return Err("Picker odd last-row source selection failed");
    }
    clear_missing_picker_selection(ui);
    if ui.get_selected_screen() != "screen:10" {
        return Err("Picker refresh cleared a still-present source");
    }
    ui.set_screen_sources(Rc::new(VecModel::from(vec![first])).into());
    clear_missing_picker_selection(ui);
    if !ui.get_selected_screen().is_empty() {
        return Err("Picker refresh retained a removed source");
    }
    click(680.0, 582.0);
    if !starts.borrow().is_empty() {
        return Err("Stream picker started without a selected source");
    }
    click(450.0, 318.0);
    if ui.get_selected_screen() != "screen:7" {
        return Err("Stream picker screen selection failed");
    }
    ui.set_screen_sources_loading(true);
    click(680.0, 582.0);
    if !starts.borrow().is_empty() {
        return Err("Loading stream picker accepted a start");
    }
    ui.set_screen_sources_loading(false);
    click(680.0, 582.0);
    if starts.borrow().as_slice() != [("screen:7".to_owned(), "low".to_owned())] {
        return Err("Stream picker did not forward low quality and exact source identity");
    }
    if audio_selections.borrow().as_slice() != [true] {
        return Err("Screen audio default was not forwarded");
    }
    click(730.0, 450.0);
    if ui.get_screen_include_audio() {
        return Err("Stream audio toggle did not disable capture");
    }
    if control_selections.borrow().last() != Some(&false) {
        return Err("Stream control started without explicit consent");
    }
    click(730.0, 451.0);
    if !ui.get_screen_allow_control() {
        return Err("Stream control consent toggle did not enable permission");
    }
    ui.set_busy(true);
    click(730.0, 450.0);
    if ui.get_screen_include_audio() {
        return Err("Busy stream audio toggle changed selection");
    }
    click(680.0, 582.0);
    if starts.borrow().len() != 1 {
        return Err("Busy stream picker started duplicate capture");
    }
    ui.set_busy(false);
    click(700.0, 475.0);
    if ui.get_screen_quality() != "high" {
        return Err("Stream picker high quality pointer selection failed");
    }
    ui.set_busy(true);
    click(540.0, 475.0);
    if ui.get_screen_quality() != "high" {
        return Err("Busy stream picker changed quality");
    }
    ui.set_busy(false);
    click(540.0, 475.0);
    if ui.get_screen_quality() != "low" {
        return Err("Stream picker low quality pointer selection failed");
    }
    click(700.0, 475.0);
    click(680.0, 582.0);
    if starts.borrow().last() != Some(&("screen:7".to_owned(), "high".to_owned())) {
        return Err("Stream picker did not forward high quality");
    }
    if audio_selections.borrow().last() != Some(&false) {
        return Err("Disabled stream audio selection was not preserved");
    }
    if control_selections.borrow().last() != Some(&true) {
        return Err("Explicit stream control consent was not forwarded");
    }
    click(550.0, 254.0);
    if ui.get_screen_source_kind() != 1 || !ui.get_selected_screen().is_empty() {
        return Err("Stream picker tab retained the previous source");
    }
    click(450.0, 318.0);
    if ui.get_selected_screen() != "window:9" {
        return Err("Stream picker window selection failed");
    }
    click(680.0, 582.0);
    if starts.borrow().last() != Some(&("window:9".to_owned(), "high".to_owned())) {
        return Err("Stream picker window start changed quality or source identity");
    }
    if audio_selections.borrow().last() != Some(&false) {
        return Err("Window tab reset explicit audio selection");
    }
    click(730.0, 450.0);
    if !ui.get_screen_include_audio() {
        return Err("Window audio toggle did not enable capture");
    }
    click(730.0, 254.0);
    if ui.get_screen_source_kind() != 2
        || !ui.get_selected_screen().is_empty()
        || *camera_queries.borrow() != 1
    {
        return Err("Stream picker camera tab did not reset selection and request inventory");
    }
    ui.set_camera_sources_loading(true);
    click(450.0, 318.0);
    click(680.0, 582.0);
    if starts.borrow().last() != Some(&("window:9".to_owned(), "high".to_owned()))
        || !ui.get_selected_screen().is_empty()
    {
        return Err("Loading camera picker accepted source or start");
    }
    ui.set_camera_sources_loading(false);
    click(450.0, 318.0);
    if ui.get_selected_screen() != "camera:exact-device-id:with:colons" {
        return Err("Camera picker lost exact device identity");
    }
    click(680.0, 582.0);
    if starts.borrow().last()
        != Some(&(
            "camera:exact-device-id:with:colons".to_owned(),
            "camera".to_owned(),
        ))
    {
        return Err("Camera picker forwarded desktop quality or changed device identity");
    }
    if audio_selections.borrow().last() != Some(&false) {
        return Err("Camera started isolated desktop audio");
    }
    if control_selections.borrow().last() != Some(&false) {
        return Err("Camera started remote input control");
    }
    let started = starts.borrow().len();
    ui.set_busy(true);
    click(550.0, 254.0);
    click(680.0, 582.0);
    if ui.get_screen_source_kind() != 2 || starts.borrow().len() != started {
        return Err("Busy camera picker changed tab or started duplicate capture");
    }
    ui.set_busy(false);
    ui.set_camera_sources(ModelRc::default());
    ui.set_selected_screen("".into());
    click(680.0, 582.0);
    if starts.borrow().len() != started {
        return Err("Empty camera picker started a capture");
    }
    let cancelled = *camera_cancellations.borrow();
    click(430.0, 582.0);
    if ui.get_modal() != 0 || *camera_cancellations.borrow() <= cancelled {
        return Err("Stream picker cancel failed");
    }
    clear_picker_rows(ui);
    if ui.get_screen_sources().row_count() != 0
        || ui.get_window_sources().row_count() != 0
        || ui.get_camera_sources().row_count() != 0
    {
        return Err("Closed picker retained source previews");
    }
    Ok(())
}

fn check_audio_interactions(ui: &NativeApp) -> Result<(), &'static str> {
    use slint::platform::{Key, PointerEventButton, WindowEvent};
    ui.set_reduced_motion(true);
    let settings = Rc::new(RefCell::new(AudioSettings::default()));
    let edited = Rc::new(RefCell::new(Vec::<String>::new()));
    let state = settings.clone();
    let observed = edited.clone();
    let weak = ui.as_weak();
    ui.on_audio_setting_changed(move |key, value| {
        let value = if matches!(
            key.as_str(),
            "selectedInput" | "selectedOutput" | "micThresholdMode" | "smartNoiseModel"
        ) {
            Some(json!(value.as_str()))
        } else {
            serde_json::from_str(value.as_str()).ok()
        };
        let next = value.and_then(|value| state.borrow().edited(key.as_str(), value));
        if let (Some(next), Some(ui)) = (next, weak.upgrade()) {
            observed.borrow_mut().push(key.to_string());
            apply_audio_settings(&ui, &next);
            *state.borrow_mut() = next;
        }
    });
    apply_audio_settings(ui, &settings.borrow());
    apply_audio_devices(ui, &[], &settings.borrow());
    ui.set_settings_tab(1);
    ui.set_settings_scroll_y(0.0);
    ui.set_modal(3);
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
    let key = |key: Key| {
        ui.window()
            .dispatch_event(WindowEvent::KeyPressed { text: key.into() });
        ui.window()
            .dispatch_event(WindowEvent::KeyReleased { text: key.into() });
    };
    let offset = ui.get_settings_dialog_y() - 78.0;
    click(550.0, 464.0 + offset);
    key(Key::RightArrow);
    if settings.borrow().input_volume != 105.0 {
        return Err("Audio slider pointer/focus/arrow step failed");
    }
    key(Key::Home);
    if settings.borrow().input_volume != 0.0 {
        return Err("Audio slider minimum failed");
    }
    key(Key::End);
    if settings.borrow().input_volume != 200.0 {
        return Err("Audio slider maximum failed");
    }
    click(550.0, 564.0 + offset);
    key(Key::LeftArrow);
    if settings.borrow().output_volume != 95.0 || settings.borrow().input_volume != 200.0 {
        return Err("Output slider changed the wrong audio preference");
    }
    click(734.0, 656.0 + offset);
    if !settings.borrow().ultra_low_latency {
        return Err("Ultra-low-latency toggle failed");
    }
    ui.set_settings_scroll_y(-380.0);
    let count = edited.borrow().len();
    click(734.0, 356.0 + offset);
    if ui.get_settings_scroll_y() > -300.0 {
        return Err("Audio settings scroll did not expose the disabled controls");
    }
    if edited.borrow().len() != count {
        return Err("Disabled heavy audio control accepted an edit");
    }
    click(758.0, 116.0 + offset);
    if ui.get_modal() != 0 {
        return Err("Audio settings close action failed");
    }
    ui.set_voice_active(true);
    let peer = zabor_native::auth::User {
        id: "volume-fixture-peer".into(),
        ..Default::default()
    };
    ui.set_voice_users(friend_rows(&[peer]));
    ui.set_volume_peer("volume-fixture-peer".into());
    ui.set_modal(8);
    reconcile_volume_modal(ui);
    if ui.get_modal() != 8 {
        return Err("Active peer volume dialog closed prematurely");
    }
    ui.set_voice_users(ModelRc::default());
    reconcile_volume_modal(ui);
    if ui.get_modal() != 0 || !ui.get_volume_peer().is_empty() {
        return Err("Departed peer retained a stale volume dialog");
    }
    Ok(())
}

fn check_mic_test_interactions(ui: &NativeApp) -> Result<(), &'static str> {
    use slint::platform::{Key, PointerEventButton, WindowEvent};
    use zabor_native::mic_test::{MicTest, MONITOR_SETTLE, PIPELINE_FLUSH};
    let test = Arc::new(MicTest::default());
    let weak = ui.as_weak();
    let local = test.clone();
    ui.on_mic_test_context_changed(move || {
        if let Some(ui) = weak.upgrade() {
            local.configure(
                ui.get_authenticated()
                    && ui.get_modal() == 3
                    && ui.get_audio_settings().noise_suppression,
                ui.get_settings_tab() == 1,
            );
            apply_mic_test(&ui, local.snapshot(std::time::Instant::now()));
        }
    });
    let weak = ui.as_weak();
    let local = test.clone();
    ui.on_mic_test_record(move || {
        if let Some(ui) = weak.upgrade().filter(|ui| ui.get_mic_test_active()) {
            local.begin(std::time::Instant::now());
            apply_mic_test(&ui, local.snapshot(std::time::Instant::now()));
        }
    });
    let weak = ui.as_weak();
    let local = test.clone();
    ui.on_mic_test_toggle(move || {
        if let Some(ui) = weak.upgrade().filter(|ui| ui.get_mic_test_active()) {
            if local.snapshot(std::time::Instant::now()).playing {
                local.pause();
            } else {
                local.play(None);
            }
            apply_mic_test(&ui, local.snapshot(std::time::Instant::now()));
        }
    });
    let weak = ui.as_weak();
    let local = test.clone();
    ui.on_mic_test_seek(move |seconds| {
        if let Some(ui) = weak.upgrade().filter(|ui| ui.get_mic_test_active()) {
            local.seek(f64::from(seconds));
            apply_mic_test(&ui, local.snapshot(std::time::Instant::now()));
        }
    });
    apply_audio_settings(ui, &AudioSettings::default());
    ui.set_settings_tab(1);
    ui.set_modal(3);
    ui.invoke_mic_test_context_changed();
    ui.set_mic_test_revealed(true);
    let pointer = |pressed, x, y| {
        let position = slint::LogicalPosition::new(x, y);
        ui.window().dispatch_event(if pressed {
            WindowEvent::PointerPressed {
                position,
                button: PointerEventButton::Left,
            }
        } else {
            WindowEvent::PointerReleased {
                position,
                button: PointerEventButton::Left,
            }
        });
    };
    let click = |x, y| {
        pointer(true, x, y);
        pointer(false, x, y);
    };
    let key = |key: Key| {
        ui.window()
            .dispatch_event(WindowEvent::KeyPressed { text: key.into() });
        ui.window()
            .dispatch_event(WindowEvent::KeyReleased { text: key.into() });
    };
    click(550.0, ui.get_mic_test_panel_y() + 18.0);
    if ui.get_mic_test().stage != "arming" {
        return Err("Microphone record pointer callback failed");
    }
    let token = test
        .capture_token()
        .ok_or("Microphone capture token missing")?;
    click(550.0, ui.get_mic_test_panel_y() + 18.0);
    if test.capture_token() != Some(token) {
        return Err("Disabled recording button started a duplicate capture");
    }
    let complete = |sample: f32| -> Result<(), &'static str> {
        let token = test
            .capture_token()
            .ok_or("Microphone retake token missing")?;
        let now = std::time::Instant::now();
        test.prepared(token, now);
        test.snapshot(now + MONITOR_SETTLE);
        for index in 0..500 {
            test.capture(
                token,
                &[sample; 480],
                -24.0,
                now + MONITOR_SETTLE + PIPELINE_FLUSH + Duration::from_millis(index * 10),
            );
        }
        apply_mic_test(ui, test.snapshot(now + Duration::from_secs(6)));
        Ok(())
    };
    complete(0.1)?;
    if ui.get_mic_test().stage != "ready" || !ui.get_mic_test().playing {
        return Err("Microphone ready player failed");
    }
    let y = ui.get_mic_test_panel_y() + 18.0;
    pointer(true, 568.0, y);
    ui.window().dispatch_event(WindowEvent::PointerMoved {
        position: slint::LogicalPosition::new(615.0, y),
    });
    if test.snapshot(std::time::Instant::now()).position_seconds != 0.0 {
        return Err("Microphone scrub committed before pointer-up");
    }
    pointer(false, 615.0, y);
    if (ui.get_mic_test().position - 4.95).abs() > 0.001 {
        return Err("Microphone pointer-up seek/tail bound failed");
    }
    key(Key::LeftArrow);
    if (ui.get_mic_test().position - 4.45).abs() > 0.001 {
        return Err("Microphone half-second keyboard seek failed");
    }
    key(Key::RightArrow);
    pointer(true, 548.0, y);
    pointer(false, 580.0, y);
    if (ui.get_mic_test().position - 69.0 / 104.0 * 5.0).abs() > 0.001 {
        return Err("Microphone release without pointer-move used a stale seek target");
    }
    test.seek(1.5);
    apply_mic_test(ui, test.snapshot(std::time::Instant::now()));
    click(490.0, ui.get_mic_test_panel_y() + 18.0);
    if ui.get_mic_test().playing {
        return Err("Microphone pause button failed");
    }
    key(Key::Return);
    if !ui.get_mic_test().playing {
        return Err("Microphone keyboard playback failed");
    }
    ui.set_settings_tab(0);
    ui.invoke_mic_test_context_changed();
    if ui.get_mic_test().playing || ui.get_mic_test().duration != 5.0 {
        return Err("Microphone tab hide did not pause/retain clip");
    }
    ui.set_settings_tab(1);
    ui.invoke_mic_test_context_changed();
    ui.set_mic_test_revealed(true);
    if ui.get_mic_test().playing {
        return Err("Microphone tab show caused late autoplay");
    }
    click(550.0, ui.get_mic_test_panel_y() + 51.0);
    if ui.get_mic_test().stage != "arming" || ui.get_mic_test().duration != 0.0 {
        return Err("Microphone retake did not reset clip");
    }
    complete(0.0)?;
    if ui.get_mic_test().issue != "silent" {
        return Err("Microphone silence state failed");
    }
    let audio = AudioSettings {
        noise_suppression: false,
        ..AudioSettings::default()
    };
    apply_audio_settings(ui, &audio);
    ui.invoke_mic_test_context_changed();
    if test.snapshot(std::time::Instant::now()).stored_samples != 0 {
        return Err("Microphone disable did not dispose buffer");
    }
    ui.set_modal(0);
    ui.invoke_mic_test_context_changed();
    if test.has_demand() {
        return Err("Microphone close retained local demand");
    }
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
    let mut data = notification_fixture();
    apply_bootstrap(ui, &data);
    ui.set_show_notifications(true);
    if ui.get_notifications().row_count() != 2 {
        return Err("Native notification list did not include requests and invites");
    }
    let invite = ui
        .get_notifications()
        .row_data(1)
        .ok_or("Native invite missing")?;
    if ui
        .invoke_tr_name(
            "main.notifications.channelInvite".into(),
            invite.sender,
            ui.get_language(),
        )
        .contains("{{name}}")
    {
        return Err("Native invitation label did not resolve the sender");
    }
    resolve_notification(
        ui,
        &mut data,
        NotificationKind::Friend,
        "fixture-request",
        false,
    );
    if ui.get_notifications().row_count() != 1 || !ui.get_show_notifications() {
        return Err("Friend request response changed the wrong native notification");
    }
    resolve_notification(
        ui,
        &mut data,
        NotificationKind::Channel,
        "fixture-invite",
        true,
    );
    if ui.get_notifications().row_count() != 0 || ui.get_show_notifications() {
        return Err("Accepted invitation did not close the native notification panel");
    }
    if ui.get_modal() != 6 || ui.get_pending_channel() != "fixture-invite" {
        return Err("Active voice session did not require channel switch confirmation");
    }
    apply_hub_event(
        ui,
        &mut data,
        HubEvent {
            target: "ChannelDeleted".into(),
            arguments: vec![json!("fixture-invite")],
        },
    );
    if ui.get_modal() != 0 || !ui.get_pending_channel().is_empty() {
        return Err("Deleted channel remained in native switch confirmation");
    }
    Ok(())
}

fn notification_fixture() -> Bootstrap {
    Bootstrap {
        user: zabor_native::auth::User {
            id: "fixture-owner".into(),
            username: "local.fixture".into(),
            display_name: "Local fixture".into(),
            avatar_color: "#c81e70".into(),
            ..Default::default()
        },
        requests: vec![zabor_native::auth::User {
            id: "fixture-request".into(),
            username: "alex.fixture".into(),
            display_name: "Alex".into(),
            avatar_color: "#5875a4".into(),
            ..Default::default()
        }],
        invites: vec![zabor_native::auth::ChannelInvite {
            sender_id: "fixture-sender".into(),
            sender_name: "Мария".into(),
            channel_id: "fixture-invite".into(),
            channel_name: "Общий канал".into(),
        }],
        ..Default::default()
    }
}

fn file_fixture() -> Vec<ChatMessage> {
    ["missing", "transferring", "available", "failed", "deleted"].iter().enumerate().map(|(index, state)| ChatMessage {
        id: format!("fixture-file-{index}"), owner_id: "fixture-owner".into(), conversation_id: "fixture-request".into(), sender_id: "fixture-request".into(), target_id: "fixture-owner".into(), kind: "file".into(), text: String::new(), created_at: 0, sequence: index as i64, delivery: "read".into(),
        file: Some(json!({"name":format!("Документ {}.pdf", index + 1),"size":2468000,"sha256":"0".repeat(64),"storedName":if *state == "available" { Some("fixture.bin") } else { None },"savedPath":null,"transferState":state,"progress":if *state == "transferring" { 0.43 } else { 0.0 }})),
    }).collect()
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
    ui.set_notifications(
        Rc::new(VecModel::from(
            data.requests
                .iter()
                .map(|user| NotificationItem {
                    id: user.id.clone().into(),
                    kind: 0,
                    name: user.display_name.clone().into(),
                    sender: "".into(),
                    color: color(&user.avatar_color),
                    avatar: user
                        .avatar_base64
                        .as_deref()
                        .map(decode_image)
                        .unwrap_or_default(),
                })
                .chain(data.invites.iter().map(|invite| NotificationItem {
                    id: invite.channel_id.clone().into(),
                    kind: 1,
                    name: invite.channel_name.clone().into(),
                    sender: invite.sender_name.clone().into(),
                    color: slint::Color::default(),
                    avatar: slint::Image::default(),
                }))
                .collect::<Vec<_>>(),
        ))
        .into(),
    );
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

fn chat_line(message: ChatMessage, owner: &str) -> ChatLine {
    let file = message.file.as_ref();
    let size = file.and_then(|file| file["size"].as_u64()).unwrap_or(0);
    let (size_text, size_unit) = if size < 1024 {
        (size.to_string(), "b")
    } else if size < 1024 * 1024 {
        (format!("{:.1}", size as f64 / 1024.0), "kb")
    } else if size < 1024 * 1024 * 1024 {
        (format!("{:.1}", size as f64 / 1048576.0), "mb")
    } else {
        (format!("{:.1}", size as f64 / 1073741824.0), "gb")
    };
    ChatLine {
        id: message.id.into(),
        text: if let Some(file) = file {
            file["name"].as_str().unwrap_or("").into()
        } else {
            message.text.into()
        },
        mine: message.sender_id == owner,
        delivery: message.delivery.into(),
        file: file.is_some(),
        size_text: size_text.into(),
        size_unit: size_unit.into(),
        transfer: file
            .and_then(|file| file["transferState"].as_str())
            .unwrap_or("")
            .into(),
        progress: (file
            .and_then(|file| file["progress"].as_f64())
            .unwrap_or(0.0)
            .clamp(0.0, 1.0)
            * 100.0) as i32,
        cached: file.is_some_and(|file| file["storedName"].is_string()),
        saved: file.is_some_and(|file| file["savedPath"].is_string()),
    }
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
    chat.set_file_root(&profile.root().join("chat-files"))?;
    chat.prune(now_ms(), &profile.root().join("chat-files"))?;
    Ok(chat)
}

async fn send_chat_packets(
    work: &SessionWork<'_>,
    hub: &HubConnection,
    transport: Option<&ChatTransport>,
    packets: Vec<zabor_native::chat::OutboundPacket>,
) {
    for packet in packets {
        if !work.is_current() {
            break;
        }
        if transport.is_some_and(|transport| transport.send_control(&packet)) {
            continue;
        }
        if !matches!(
            work.run(hub.send(
                "SendChatRelay",
                vec![json!(packet.peer), json!(packet.payload)],
            ))
            .await,
            Some(Ok(()))
        ) {
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
    if event.target == "ChannelDeleted"
        && event.arguments.first().and_then(serde_json::Value::as_str)
            == Some(ui.get_pending_channel().as_str())
    {
        ui.set_pending_channel("".into());
        if ui.get_modal() == 6 {
            ui.set_modal(0);
        }
    }
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

fn resolve_notification(
    ui: &NativeApp,
    data: &mut Bootstrap,
    kind: NotificationKind,
    id: &str,
    accept: bool,
) {
    match kind {
        NotificationKind::Friend => data.requests.retain(|user| user.id != id),
        NotificationKind::Channel => {
            let name = data
                .invites
                .iter()
                .find(|invite| invite.channel_id == id)
                .map(|invite| invite.channel_name.clone())
                .unwrap_or_default();
            data.invites.retain(|invite| invite.channel_id != id);
            if accept {
                ui.set_show_notifications(false);
                if ui.get_voice_active() {
                    ui.set_pending_channel(id.into());
                    ui.set_pending_channel_name(name.into());
                    ui.set_modal(6);
                } else {
                    ui.set_active_tab(0);
                    ui.set_selected_name(name.into());
                }
            }
        }
    }
    apply_bootstrap(ui, data);
    ui.set_busy(false);
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
