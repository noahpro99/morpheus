/*
 * Morpheus patch: see mf_common.h.
 */

#include <cstdio>

#include "rtc_base/logging.h"

#include <windows.h>
// The GUIDs used here, defined (selectany) so nothing links mfuuid.lib.
#include <initguid.h>
#include "mf_common.h"

namespace livekit_ffi {
namespace mf {

const MfApi* Mf() {
  static const MfApi* api = []() -> const MfApi* {
    HMODULE dll = LoadLibraryExW(L"mfplat.dll", nullptr,
                                 LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!dll) {
      RTC_LOG(LS_INFO) << "Media Foundation: mfplat.dll is not here";
      return nullptr;
    }
    auto* found = new MfApi();
    found->startup = reinterpret_cast<decltype(&::MFStartup)>(
        GetProcAddress(dll, "MFStartup"));
    found->shutdown = reinterpret_cast<decltype(&::MFShutdown)>(
        GetProcAddress(dll, "MFShutdown"));
    found->enum_ex = reinterpret_cast<decltype(&::MFTEnumEx)>(
        GetProcAddress(dll, "MFTEnumEx"));
    found->create_media_type = reinterpret_cast<decltype(&::MFCreateMediaType)>(
        GetProcAddress(dll, "MFCreateMediaType"));
    found->create_sample = reinterpret_cast<decltype(&::MFCreateSample)>(
        GetProcAddress(dll, "MFCreateSample"));
    found->create_memory_buffer =
        reinterpret_cast<decltype(&::MFCreateMemoryBuffer)>(
            GetProcAddress(dll, "MFCreateMemoryBuffer"));
    found->create_dxgi_device_manager =
        reinterpret_cast<decltype(&::MFCreateDXGIDeviceManager)>(
            GetProcAddress(dll, "MFCreateDXGIDeviceManager"));
    if (!found->startup || !found->shutdown || !found->enum_ex ||
        !found->create_media_type || !found->create_sample ||
        !found->create_memory_buffer) {
      RTC_LOG(LS_WARNING) << "Media Foundation: mfplat.dll lacks a function";
      delete found;
      return nullptr;
    }
    return found;
  }();
  return api;
}

void EnsureCom() {
  struct Com {
    HRESULT hr = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    ~Com() {
      if (SUCCEEDED(hr))
        CoUninitialize();
    }
  };
  static thread_local Com com;
  (void)com;
}

MfStarted::MfStarted() {
  const MfApi* mf = Mf();
  ok_ = mf && SUCCEEDED(mf->startup(MF_VERSION, MFSTARTUP_LITE));
}

MfStarted::~MfStarted() {
  if (ok_)
    Mf()->shutdown();
}

std::string Utf8(const wchar_t* text) {
  if (!text)
    return {};
  int n = WideCharToMultiByte(CP_UTF8, 0, text, -1, nullptr, 0, nullptr,
                              nullptr);
  if (n <= 1)
    return {};
  std::string out(static_cast<size_t>(n - 1), '\0');
  WideCharToMultiByte(CP_UTF8, 0, text, -1, out.data(), n, nullptr, nullptr);
  return out;
}

std::string Hex(HRESULT hr) {
  char text[16];
  snprintf(text, sizeof(text), "0x%08lx", static_cast<unsigned long>(hr));
  return text;
}

std::string FriendlyName(IMFActivate* activate) {
  wchar_t* name = nullptr;
  UINT32 length = 0;
  if (FAILED(activate->GetAllocatedString(MFT_FRIENDLY_NAME_Attribute, &name,
                                          &length)))
    return "unnamed MFT";
  std::string out = Utf8(name);
  CoTaskMemFree(name);
  return out;
}

std::vector<Microsoft::WRL::ComPtr<IMFActivate>> FindMfts(const GUID& category,
                                                          const GUID& input,
                                                          const GUID& output,
                                                          UINT32 flags) {
  std::vector<Microsoft::WRL::ComPtr<IMFActivate>> out;
  const MfApi* mf = Mf();
  if (!mf)
    return out;
  MFT_REGISTER_TYPE_INFO in = {MFMediaType_Video, input};
  MFT_REGISTER_TYPE_INFO to = {MFMediaType_Video, output};
  IMFActivate** found = nullptr;
  UINT32 count = 0;
  HRESULT hr = mf->enum_ex(category, flags, &in, &to, &found, &count);
  if (FAILED(hr) || !found)
    return out;
  for (UINT32 i = 0; i < count; ++i) {
    out.emplace_back(found[i]);
    found[i]->Release();
  }
  CoTaskMemFree(found);
  return out;
}

}  // namespace mf
}  // namespace livekit_ffi
