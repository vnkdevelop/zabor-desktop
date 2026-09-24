#include <napi.h>
#include <windows.h>
#include <mmdeviceapi.h>
#include <audioclient.h>
#include <string>

#pragma comment(lib, "Mmdevapi.lib")
#pragma comment(lib, "Ole32.lib")

namespace {

struct PeriodInfo {
  bool ok = false;
  std::string error;
  unsigned int sampleRate = 0;
  unsigned int channels = 0;
  unsigned int defaultFrames = 0;
  unsigned int fundamentalFrames = 0;
  unsigned int minFrames = 0;
  unsigned int maxFrames = 0;
};

static PeriodInfo QueryEndpoint(IMMDeviceEnumerator* enumerator, EDataFlow flow) {
  PeriodInfo info;
  IMMDevice* device = nullptr;
  HRESULT hr = enumerator->GetDefaultAudioEndpoint(flow, eConsole, &device);
  if (FAILED(hr) || device == nullptr) {
    info.error = "no default endpoint";
    return info;
  }

  IAudioClient3* client = nullptr;
  hr = device->Activate(__uuidof(IAudioClient3), CLSCTX_ALL, nullptr,
                        reinterpret_cast<void**>(&client));
  if (FAILED(hr) || client == nullptr) {
    device->Release();
    info.error = "iaudioclient3 unavailable";
    return info;
  }

  WAVEFORMATEX* mix = nullptr;
  hr = client->GetMixFormat(&mix);
  if (FAILED(hr) || mix == nullptr) {
    client->Release();
    device->Release();
    info.error = "getmixformat failed";
    return info;
  }

  UINT32 def = 0, fund = 0, mn = 0, mx = 0;
  hr = client->GetSharedModeEnginePeriod(mix, &def, &fund, &mn, &mx);
  if (SUCCEEDED(hr)) {
    info.ok = true;
    info.sampleRate = mix->nSamplesPerSec;
    info.channels = mix->nChannels;
    info.defaultFrames = def;
    info.fundamentalFrames = fund;
    info.minFrames = mn;
    info.maxFrames = mx;
  } else {
    info.error = "getsharedmodeengineperiod failed";
  }

  CoTaskMemFree(mix);
  client->Release();
  device->Release();
  return info;
}

static Napi::Object ToJs(Napi::Env env, const PeriodInfo& p) {
  Napi::Object o = Napi::Object::New(env);
  o.Set("ok", Napi::Boolean::New(env, p.ok));
  if (!p.ok) o.Set("error", Napi::String::New(env, p.error));
  o.Set("sampleRate", Napi::Number::New(env, p.sampleRate));
  o.Set("channels", Napi::Number::New(env, p.channels));
  o.Set("defaultPeriodFrames", Napi::Number::New(env, p.defaultFrames));
  o.Set("fundamentalPeriodFrames", Napi::Number::New(env, p.fundamentalFrames));
  o.Set("minPeriodFrames", Napi::Number::New(env, p.minFrames));
  o.Set("maxPeriodFrames", Napi::Number::New(env, p.maxFrames));
  double sr = p.sampleRate > 0 ? static_cast<double>(p.sampleRate) : 48000.0;
  o.Set("defaultPeriodMs", Napi::Number::New(env, p.defaultFrames * 1000.0 / sr));
  o.Set("minPeriodMs", Napi::Number::New(env, p.minFrames * 1000.0 / sr));
  return o;
}

Napi::Value Probe(const Napi::CallbackInfo& info) {
  Napi::Env env = info.Env();
  Napi::Object result = Napi::Object::New(env);

  HRESULT hrInit = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
  bool didInit = SUCCEEDED(hrInit);

  IMMDeviceEnumerator* enumerator = nullptr;
  HRESULT hr = CoCreateInstance(__uuidof(MMDeviceEnumerator), nullptr, CLSCTX_ALL,
                                __uuidof(IMMDeviceEnumerator),
                                reinterpret_cast<void**>(&enumerator));
  if (FAILED(hr) || enumerator == nullptr) {
    result.Set("ok", Napi::Boolean::New(env, false));
    result.Set("error", Napi::String::New(env, "mmdeviceenumerator failed"));
    if (didInit) CoUninitialize();
    return result;
  }

  PeriodInfo cap = QueryEndpoint(enumerator, eCapture);
  PeriodInfo ren = QueryEndpoint(enumerator, eRender);
  enumerator->Release();
  if (didInit) CoUninitialize();

  result.Set("ok", Napi::Boolean::New(env, cap.ok || ren.ok));
  result.Set("capture", ToJs(env, cap));
  result.Set("render", ToJs(env, ren));
  return result;
}

}

static Napi::Object Init(Napi::Env env, Napi::Object exports) {
  exports.Set("probe", Napi::Function::New(env, Probe));
  return exports;
}

NODE_API_MODULE(zabor_low_latency_audio, Init)
