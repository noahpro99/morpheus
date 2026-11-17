/*
 * Morpheus patch: what the Media Foundation H.264 encoder and decoder share
 * (Windows only): mfplat.dll loaded at run time, COM on the calling thread,
 * MFStartup / MFShutdown, MFT enumeration and small text helpers.
 */

#pragma once

#include <string>
#include <vector>

#include <windows.h>
#include <mfapi.h>
#include <mfidl.h>
#include <mftransform.h>
#include <wrl/client.h>

namespace livekit_ffi {
namespace mf {

// mfplat.dll's functions, loaded at run time (a Windows N without the Media
// Feature Pack has none and still starts).
struct MfApi {
  decltype(&::MFStartup) startup = nullptr;
  decltype(&::MFShutdown) shutdown = nullptr;
  decltype(&::MFTEnumEx) enum_ex = nullptr;
  decltype(&::MFCreateMediaType) create_media_type = nullptr;
  decltype(&::MFCreateSample) create_sample = nullptr;
  decltype(&::MFCreateMemoryBuffer) create_memory_buffer = nullptr;
  // Optional (decoding on the GPU only).
  decltype(&::MFCreateDXGIDeviceManager) create_dxgi_device_manager = nullptr;
};

// mfplat.dll, looked for once; null when it is not there.
const MfApi* Mf();

// COM on this thread for as long as the thread lives (MFTs are free
// threaded; a thread already in another apartment keeps it).
void EnsureCom();

// MFStartup / MFShutdown, balanced.
class MfStarted {
 public:
  MfStarted();
  ~MfStarted();
  MfStarted(const MfStarted&) = delete;
  MfStarted& operator=(const MfStarted&) = delete;
  bool ok() const { return ok_; }

 private:
  bool ok_ = false;
};

std::string Utf8(const wchar_t* text);
std::string Hex(HRESULT hr);
std::string FriendlyName(IMFActivate* activate);

// The MFTs of `category` from `input` to `output` with `flags`
// (MFT_ENUM_FLAG_*), best first.
std::vector<Microsoft::WRL::ComPtr<IMFActivate>> FindMfts(const GUID& category,
                                                          const GUID& input,
                                                          const GUID& output,
                                                          UINT32 flags);

}  // namespace mf
}  // namespace livekit_ffi
