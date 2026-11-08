#include "zabor-camera/src/lib.rs.h"
#include <Windows.h>
#include <mfapi.h>
#include <mfidl.h>
#include <mfreadwrite.h>
#include <mferror.h>
#include <wrl/client.h>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstring>
#include <limits>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>

namespace zabor::camera {
namespace {
using Microsoft::WRL::ComPtr;
using namespace std::chrono_literals;

void checked(HRESULT result) {
    if (FAILED(result)) throw std::runtime_error("native camera operation failed");
}

struct Image {
    std::uint32_t width = 0;
    std::uint32_t height = 0;
    std::vector<std::uint8_t> bgra;
};

Image copy_rows(const BYTE* base, std::size_t length, std::uintptr_t top,
                LONG stride, UINT32 width, UINT32 height) {
    if (width == 0 || height == 0 || width > 3840 || height > 2160) {
        throw std::runtime_error("unsupported camera dimensions");
    }
    const auto pitch = std::abs(static_cast<std::int64_t>(stride));
    const auto row_bytes = static_cast<std::uint64_t>(width) * 4;
    if (pitch < static_cast<std::int64_t>(row_bytes) || pitch > 3840 * 4 + 65536 ||
        static_cast<std::uint64_t>(pitch) * (height - 1) + row_bytes > length) {
        throw std::runtime_error("invalid camera buffer");
    }
    const auto start = reinterpret_cast<std::uintptr_t>(base);
    if (start > std::numeric_limits<std::uintptr_t>::max() - length) {
        throw std::runtime_error("invalid camera buffer bounds");
    }
    Image image{width, height, std::vector<std::uint8_t>(static_cast<std::size_t>(row_bytes * height))};
    for (UINT32 row = 0; row < height; ++row) {
        const auto offset = static_cast<std::int64_t>(row) * stride;
        const auto distance = static_cast<std::uint64_t>(std::abs(offset));
        if ((offset < 0 && top < distance) ||
            (offset >= 0 && top > std::numeric_limits<std::uintptr_t>::max() - distance)) {
            throw std::runtime_error("invalid camera row offset");
        }
        const auto address = offset < 0 ? top - distance : top + distance;
        if (address < start || address > start + length || row_bytes > start + length - address) {
            throw std::runtime_error("invalid camera row bounds");
        }
        std::memcpy(image.bgra.data() + row * row_bytes,
                    reinterpret_cast<const void*>(address), static_cast<std::size_t>(row_bytes));
    }
    for (std::size_t index = 3; index < image.bgra.size(); index += 4) image.bgra[index] = 255;
    return image;
}

Frame into_frame(Image image) {
    Frame result{};
    result.width = image.width;
    result.height = image.height;
    result.bgra = allocate_pixels(image.bgra.size());
    if (result.bgra.size() != image.bgra.size()) return Frame{};
    if (!image.bgra.empty()) std::memcpy(result.bgra.data(), image.bgra.data(), image.bgra.size());
    return result;
}

struct DeviceRecord {
    std::string id;
    std::string name;
};

struct State {
    std::atomic<bool> stop{false};
    std::atomic<bool> terminated{false};
    std::mutex mutex;
    std::condition_variable wake;
    OwnerStatus status{};
    Image image;
    std::vector<DeviceRecord> devices;

    State() { status.owner_thread_consistent = true; }
};

struct Inbox {
    std::mutex mutex;
    std::condition_variable wake;
    ComPtr<IMFSample> sample;
    HRESULT error = S_OK;
    DWORD flags = 0;
    bool received = false;
    bool accepting = true;
    bool flushed = false;
    bool released = false;
};

class ReaderCallback final : public IMFSourceReaderCallback {
    std::atomic<ULONG> references_{1};
    std::shared_ptr<Inbox> inbox_;
public:
    explicit ReaderCallback(std::shared_ptr<Inbox> inbox) : inbox_(std::move(inbox)) {}

    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id, void** output) override {
        if (!output) return E_POINTER;
        *output = nullptr;
        if (id == IID_IUnknown || id == __uuidof(IMFSourceReaderCallback)) {
            *output = static_cast<IMFSourceReaderCallback*>(this);
            AddRef();
            return S_OK;
        }
        return E_NOINTERFACE;
    }

    ULONG STDMETHODCALLTYPE AddRef() override { return ++references_; }
    ULONG STDMETHODCALLTYPE Release() override {
        const auto count = --references_;
        if (count == 0) delete this;
        return count;
    }

    HRESULT STDMETHODCALLTYPE OnReadSample(HRESULT result, DWORD, DWORD flags,
                                            LONGLONG, IMFSample* sample) override {
        {
            std::lock_guard lock(inbox_->mutex);
            if (!inbox_->accepting) return S_OK;
            if (inbox_->received) {
                inbox_->error = E_UNEXPECTED;
            } else {
                inbox_->error = result;
                inbox_->flags = flags;
                inbox_->sample = sample;
            }
            inbox_->received = true;
        }
        inbox_->wake.notify_all();
        return S_OK;
    }

    HRESULT STDMETHODCALLTYPE OnEvent(DWORD, IMFMediaEvent* event) override {
        HRESULT result = S_OK;
        if (event && SUCCEEDED(event->GetStatus(&result)) && FAILED(result)) {
            {
                std::lock_guard lock(inbox_->mutex);
                if (inbox_->accepting) inbox_->error = result;
            }
            inbox_->wake.notify_all();
        }
        return S_OK;
    }

    HRESULT STDMETHODCALLTYPE OnFlush(DWORD) override {
        {
            std::lock_guard lock(inbox_->mutex);
            inbox_->flushed = true;
        }
        inbox_->wake.notify_all();
        return S_OK;
    }

private:
    ~ReaderCallback() {
        {
            std::lock_guard lock(inbox_->mutex);
            inbox_->released = true;
        }
        inbox_->wake.notify_all();
    }
};

struct Runtime {
    bool com = false;
    bool mf = false;
    Runtime() {
        checked(CoInitializeEx(nullptr, COINIT_MULTITHREADED));
        com = true;
        const auto result = MFStartup(MF_VERSION, MFSTARTUP_LITE);
        if (FAILED(result)) {
            CoUninitialize();
            com = false;
            checked(result);
        }
        mf = true;
    }
    bool close() {
        HRESULT result = S_OK;
        if (mf) {
            result = MFShutdown();
            mf = false;
        }
        if (com) {
            CoUninitialize();
            com = false;
        }
        return SUCCEEDED(result);
    }
    ~Runtime() { close(); }
};

std::string utf8(const std::wstring& value) {
    if (value.empty()) return {};
    const auto size = WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, value.data(),
        static_cast<int>(value.size()), nullptr, 0, nullptr, nullptr);
    if (size <= 0) throw std::runtime_error("invalid native camera name");
    std::string result(static_cast<std::size_t>(size), '\0');
    if (WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, value.data(),
        static_cast<int>(value.size()), result.data(), size, nullptr, nullptr) != size) {
        throw std::runtime_error("invalid native camera name");
    }
    return result;
}

std::wstring attribute(IMFAttributes* object, REFGUID key) {
    WCHAR* data = nullptr;
    UINT32 length = 0;
    checked(object->GetAllocatedString(key, &data, &length));
    const std::unique_ptr<WCHAR, decltype(&CoTaskMemFree)> owned(data, CoTaskMemFree);
    if (length > 32768) throw std::runtime_error("native camera name is too long");
    return std::wstring(data, length);
}

struct Activations {
    IMFActivate** items = nullptr;
    UINT32 count = 0;
    Activations() {
        ComPtr<IMFAttributes> attributes;
        checked(MFCreateAttributes(&attributes, 1));
        checked(attributes->SetGUID(MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                                   MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID));
        checked(MFEnumDeviceSources(attributes.Get(), &items, &count));
    }
    ~Activations() {
        for (UINT32 index = 0; index < count; ++index) items[index]->Release();
        CoTaskMemFree(items);
    }
};

class Backend {
protected:
    std::shared_ptr<State> state_;
    std::thread::id owner_;
    void check_owner() {
        if (owner_ != std::this_thread::get_id()) {
            std::lock_guard lock(state_->mutex);
            state_->status.owner_thread_consistent = false;
            throw std::runtime_error("native camera owner mismatch");
        }
    }
public:
    explicit Backend(std::shared_ptr<State> state) : state_(std::move(state)) {}
    virtual ~Backend() = default;
    virtual void open(const std::string& device, bool enumerate) = 0;
    virtual std::optional<Image> next() = 0;
    virtual bool close() = 0;
};

class MfCamera final : public Backend {
    std::unique_ptr<Runtime> runtime_;
    ComPtr<IMFMediaSource> source_;
    ComPtr<IMFSourceReader> reader_;
    ComPtr<ReaderCallback> callback_;
    std::shared_ptr<Inbox> inbox_ = std::make_shared<Inbox>();
    UINT32 width_ = 0;
    UINT32 height_ = 0;
    LONG stride_ = 0;

    void format() {
        ComPtr<IMFMediaType> type;
        checked(reader_->GetCurrentMediaType(static_cast<DWORD>(MF_SOURCE_READER_FIRST_VIDEO_STREAM), &type));
        GUID subtype{};
        checked(type->GetGUID(MF_MT_SUBTYPE, &subtype));
        if (subtype != MFVideoFormat_RGB32) throw std::runtime_error("invalid camera format");
        checked(MFGetAttributeSize(type.Get(), MF_MT_FRAME_SIZE, &width_, &height_));
        if (width_ == 0 || height_ == 0 || width_ > 3840 || height_ > 2160) {
            throw std::runtime_error("unsupported camera dimensions");
        }
        UINT32 stride = 0;
        if (SUCCEEDED(type->GetUINT32(MF_MT_DEFAULT_STRIDE, &stride))) {
            std::memcpy(&stride_, &stride, sizeof(stride_));
        } else {
            checked(MFGetStrideForBitmapInfoHeader(MFVideoFormat_RGB32.Data1, width_, &stride_));
        }
        if (std::abs(static_cast<std::int64_t>(stride_)) < static_cast<std::int64_t>(width_) * 4) {
            throw std::runtime_error("invalid camera stride");
        }
    }

    Image copy(IMFSample* sample) {
        ComPtr<IMFMediaBuffer> buffer;
        checked(sample->ConvertToContiguousBuffer(&buffer));
        ComPtr<IMF2DBuffer2> two_d;
        BYTE* first = nullptr;
        BYTE* base = nullptr;
        LONG stride = stride_;
        DWORD length = 0;
        const bool native_2d = SUCCEEDED(buffer.As(&two_d));
        if (native_2d) {
            checked(two_d->Lock2DSize(MF2DBuffer_LockFlags_Read, &first, &stride, &base, &length));
        } else {
            checked(buffer->Lock(&base, nullptr, &length));
            first = base;
        }
        struct Unlock {
            IMFMediaBuffer* buffer;
            IMF2DBuffer2* two_d;
            ~Unlock() { if (two_d) two_d->Unlock2D(); else buffer->Unlock(); }
        } unlock{buffer.Get(), native_2d ? two_d.Get() : nullptr};
        const auto pitch = std::abs(static_cast<std::int64_t>(stride));
        const auto row_bytes = static_cast<std::uint64_t>(width_) * 4;
        if (pitch < static_cast<std::int64_t>(row_bytes) ||
            static_cast<std::uint64_t>(pitch) * (height_ - 1) + row_bytes > length) {
            throw std::runtime_error("invalid camera buffer");
        }
        if (!native_2d && stride < 0) first += pitch * (height_ - 1);
        return copy_rows(base, length, reinterpret_cast<std::uintptr_t>(first), stride, width_, height_);
    }

    void request() {
        checked(reader_->ReadSample(static_cast<DWORD>(MF_SOURCE_READER_FIRST_VIDEO_STREAM), 0,
                                    nullptr, nullptr, nullptr, nullptr));
    }
public:
    explicit MfCamera(std::shared_ptr<State> state) : Backend(std::move(state)) {}

    void open(const std::string& device, bool enumerate) override {
        owner_ = std::this_thread::get_id();
        runtime_ = std::make_unique<Runtime>();
        if (state_->stop.load()) return;
        Activations activations;
        IMFActivate* selected = nullptr;
        std::vector<DeviceRecord> devices;
        for (UINT32 index = 0; index < activations.count; ++index) {
            if (state_->stop.load()) return;
            auto* activation = activations.items[index];
            const auto id = utf8(attribute(activation, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK));
            if (enumerate) {
                devices.push_back({id, utf8(attribute(activation, MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME))});
            } else if (!selected && (device.empty() || id == device)) {
                selected = activation;
            }
        }
        if (enumerate) {
            std::lock_guard lock(state_->mutex);
            if (!state_->stop.load()) {
                state_->devices = std::move(devices);
                state_->status.ready = true;
            }
            return;
        }
        if (!selected) throw std::runtime_error("native camera not found");
        checked(selected->ActivateObject(IID_PPV_ARGS(&source_)));
        if (state_->stop.load()) return;
        callback_.Attach(new ReaderCallback(inbox_));
        ComPtr<IMFAttributes> attributes;
        checked(MFCreateAttributes(&attributes, 2));
        checked(attributes->SetUnknown(MF_SOURCE_READER_ASYNC_CALLBACK, callback_.Get()));
        checked(attributes->SetUINT32(MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, TRUE));
        checked(MFCreateSourceReaderFromMediaSource(source_.Get(), attributes.Get(), &reader_));
        if (state_->stop.load()) return;
        checked(reader_->SetStreamSelection(static_cast<DWORD>(MF_SOURCE_READER_ALL_STREAMS), FALSE));
        checked(reader_->SetStreamSelection(static_cast<DWORD>(MF_SOURCE_READER_FIRST_VIDEO_STREAM), TRUE));
        ComPtr<IMFMediaType> output;
        checked(MFCreateMediaType(&output));
        checked(output->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video));
        checked(output->SetGUID(MF_MT_SUBTYPE, MFVideoFormat_RGB32));
        checked(reader_->SetCurrentMediaType(static_cast<DWORD>(MF_SOURCE_READER_FIRST_VIDEO_STREAM), nullptr, output.Get()));
        format();
        if (!state_->stop.load()) request();
    }

    std::optional<Image> next() override {
        check_owner();
        ComPtr<IMFSample> sample;
        DWORD flags = 0;
        {
            std::lock_guard lock(inbox_->mutex);
            checked(inbox_->error);
            if (!inbox_->received) return std::nullopt;
            flags = inbox_->flags;
            sample = std::move(inbox_->sample);
            inbox_->received = false;
        }
        if (flags & (MF_SOURCE_READERF_ENDOFSTREAM | MF_SOURCE_READERF_ERROR)) {
            throw std::runtime_error("native camera ended");
        }
        if (flags & (MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED | MF_SOURCE_READERF_NATIVEMEDIATYPECHANGED)) format();
        auto image = sample ? std::optional<Image>(copy(sample.Get())) : std::nullopt;
        sample.Reset();
        if (!state_->stop.load()) request();
        return image;
    }

    bool close() override {
        check_owner();
        bool success = true;
        ComPtr<IMFSample> remaining;
        {
            std::lock_guard lock(inbox_->mutex);
            inbox_->accepting = false;
            remaining = std::move(inbox_->sample);
        }
        remaining.Reset();
        if (reader_) {
            const auto result = reader_->Flush(static_cast<DWORD>(MF_SOURCE_READER_ALL_STREAMS));
            if (SUCCEEDED(result)) {
                std::unique_lock lock(inbox_->mutex);
                inbox_->wake.wait(lock, [&] { return inbox_->flushed; });
            } else {
                success = false;
            }
        }
        if (source_ && FAILED(source_->Shutdown())) success = false;
        reader_.Reset();
        source_.Reset();
        const bool had_callback = static_cast<bool>(callback_);
        callback_.Reset();
        if (had_callback) {
            std::unique_lock lock(inbox_->mutex);
            inbox_->wake.wait(lock, [&] { return inbox_->released; });
        }
        {
            std::lock_guard lock(state_->mutex);
            state_->status.callbacks_released = true;
        }
        if (runtime_ && !runtime_->close()) success = false;
        runtime_.reset();
        return success;
    }
};

#ifdef ZABOR_CAMERA_TEST_SOURCE
class TestCamera final : public Backend {
    std::uint8_t mode_;
    std::uint32_t delay_;
    std::shared_ptr<Inbox> inbox_ = std::make_shared<Inbox>();
    ComPtr<ReaderCallback> callback_;
    std::uint32_t frames_ = 0;
public:
    TestCamera(std::shared_ptr<State> state, std::uint8_t mode, std::uint32_t delay)
        : Backend(std::move(state)), mode_(mode), delay_(delay) {}
    void open(const std::string&, bool) override {
        owner_ = std::this_thread::get_id();
        if (mode_ == 1) std::this_thread::sleep_for(std::chrono::milliseconds(delay_));
        callback_.Attach(new ReaderCallback(inbox_));
        if (mode_ == 3) throw std::runtime_error("synthetic camera open failed");
    }
    std::optional<Image> next() override {
        check_owner();
        if (mode_ == 4 && frames_++ > 0) throw std::runtime_error("synthetic camera read failed");
        Image image{4, 2, {}};
        for (int pixel = 0; pixel < 8; ++pixel) {
            for (const auto value : {19, 37, 71, 255}) image.bgra.push_back(static_cast<std::uint8_t>(value));
        }
        return image;
    }
    bool close() override {
        check_owner();
        {
            std::lock_guard lock(inbox_->mutex);
            inbox_->accepting = false;
        }
        auto delayed = callback_;
        std::thread callback([delayed = std::move(delayed), delay = mode_ == 2 ? delay_ : 0]() mutable {
            std::this_thread::sleep_for(std::chrono::milliseconds(delay));
            delayed->OnReadSample(S_OK, 0, MF_SOURCE_READERF_STREAMTICK, 0, nullptr);
            delayed->OnFlush(0);
            delayed.Reset();
        });
        callback_.Reset();
        callback.join();
        {
            std::unique_lock lock(inbox_->mutex);
            inbox_->wake.wait(lock, [&] { return inbox_->released; });
            if (inbox_->received || inbox_->sample) return false;
        }
        {
            std::lock_guard lock(state_->mutex);
            state_->status.callbacks_released = true;
        }
        return mode_ != 5;
    }
};
#endif

void run_owner(std::shared_ptr<State> state, std::string device, bool enumerate,
               std::unique_ptr<Backend> backend) noexcept {
    {
        std::lock_guard lock(state->mutex);
        state->status.opening = true;
    }
    try {
        backend->open(device, enumerate);
        {
            std::lock_guard lock(state->mutex);
            state->status.opening = false;
        }
        while (!enumerate && !state->stop.load()) {
            auto image = backend->next();
            if (image) {
                std::lock_guard lock(state->mutex);
                if (!state->stop.load()) {
                    state->image = std::move(*image);
                    state->status.ready = true;
                }
            }
            std::unique_lock lock(state->mutex);
            state->wake.wait_for(lock, 10ms, [&] { return state->stop.load(); });
        }
    } catch (...) {
        std::lock_guard lock(state->mutex);
        state->status.opening = false;
        if (!state->stop.load()) state->status.failed = true;
    }
    try {
        if (!backend->close()) {
            std::lock_guard lock(state->mutex);
            state->status.cleanup_failed = true;
        }
    } catch (...) {
        std::lock_guard lock(state->mutex);
        state->status.cleanup_failed = true;
    }
    backend.reset();
    {
        std::lock_guard lock(state->mutex);
        state->image = {};
    }
    state->terminated.store(true, std::memory_order_release);
}

struct Entry {
    std::shared_ptr<State> state;
    std::mutex mutex;
    std::thread worker;
    explicit Entry(std::shared_ptr<State> value) : state(std::move(value)) {}
    bool reap() {
        if (!state->terminated.load(std::memory_order_acquire)) return false;
        std::lock_guard lock(mutex);
        if (worker.joinable()) worker.join();
        return true;
    }
    ~Entry() {
        state->stop.store(true);
        state->wake.notify_all();
        if (worker.joinable()) {
            if (state->terminated.load(std::memory_order_acquire)) worker.join();
            else worker.detach();
        }
    }
};

std::mutex registry_mutex;
std::map<std::uint64_t, std::shared_ptr<Entry>> registry;
std::uint64_t next_owner = 1;

std::shared_ptr<Entry> find(std::uint64_t owner) {
    std::lock_guard lock(registry_mutex);
    const auto found = registry.find(owner);
    return found == registry.end() ? nullptr : found->second;
}

std::uint64_t start(std::string device, bool enumerate, std::uint8_t mode, std::uint32_t delay) {
    auto state = std::make_shared<State>();
    auto entry = std::make_shared<Entry>(state);
    std::uint64_t owner = 0;
    {
        std::lock_guard lock(registry_mutex);
        if (next_owner == std::numeric_limits<std::uint64_t>::max()) throw std::runtime_error("camera owner exhausted");
        owner = next_owner++;
        registry.emplace(owner, entry);
    }
    try {
        std::unique_ptr<Backend> backend;
        if (mode == 255) backend = std::make_unique<MfCamera>(state);
#ifdef ZABOR_CAMERA_TEST_SOURCE
        else backend = std::make_unique<TestCamera>(state, mode, delay);
#else
        else throw std::runtime_error("invalid native camera backend");
        static_cast<void>(delay);
#endif
        entry->worker = std::thread(run_owner, state, std::move(device), enumerate, std::move(backend));
    } catch (...) {
        std::lock_guard lock(registry_mutex);
        registry.erase(owner);
        throw;
    }
    return owner;
}
}

std::uint64_t begin(rust::Str device, bool enumerate) {
    return start(std::string(device), enumerate, 255, 0);
}

OwnerStatus status(std::uint64_t owner) {
    auto entry = find(owner);
    if (!entry) return OwnerStatus{};
    const bool finished = entry->reap();
    std::lock_guard lock(entry->state->mutex);
    auto status = entry->state->status;
    status.finished = finished;
    return status;
}

Frame take_frame(std::uint64_t owner) {
    auto entry = find(owner);
    Image image;
    if (entry) {
        std::lock_guard lock(entry->state->mutex);
        if (!entry->state->stop.load() && !entry->state->status.failed &&
            !entry->state->terminated.load()) image = std::move(entry->state->image);
    }
    return into_frame(std::move(image));
}

rust::Vec<Device> take_devices(std::uint64_t owner) {
    auto entry = find(owner);
    std::vector<DeviceRecord> devices;
    if (entry) {
        std::lock_guard lock(entry->state->mutex);
        if (!entry->state->stop.load() && !entry->state->status.failed &&
            entry->state->status.ready) devices = std::move(entry->state->devices);
    }
    rust::Vec<Device> result;
    result.reserve(devices.size());
    for (auto& device : devices) result.push_back({rust::String(device.id), rust::String(device.name)});
    return result;
}

void cancel(std::uint64_t owner) noexcept {
    if (auto entry = find(owner)) {
        entry->state->stop.store(true);
        entry->state->wake.notify_all();
    }
}

void release(std::uint64_t owner) noexcept {
    std::shared_ptr<Entry> entry;
    {
        std::lock_guard lock(registry_mutex);
        const auto found = registry.find(owner);
        if (found == registry.end()) return;
        entry = std::move(found->second);
        registry.erase(found);
    }
    entry->state->stop.store(true);
    entry->state->wake.notify_all();
}

#ifdef ZABOR_CAMERA_TEST_SOURCE
std::uint64_t begin_test(std::uint8_t mode, std::uint32_t delay_ms) {
    if (mode > 5 || delay_ms > 1000) throw std::runtime_error("invalid camera fixture");
    return start({}, false, mode, delay_ms);
}

Frame test_copy_frame(rust::Slice<const std::uint8_t> data, std::uint32_t width,
                      std::uint32_t height, std::int32_t stride, std::size_t top) {
    if (top > data.size()) throw std::runtime_error("invalid camera scanline");
    const auto address = reinterpret_cast<std::uintptr_t>(data.data());
    return into_frame(copy_rows(data.data(), data.size(), address + top, stride, width, height));
}
#endif
}
