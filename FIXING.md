# Windows CI Build Fixes Needed

## Root Cause

Upgrading `openxr` from 0.16 to 0.21 pulled in a second `windows` crate version (0.54 or 0.62), conflicting with the workspace-pinned `windows = "=0.58.0"`. This causes `windows is ambiguous` errors and ~70 compilation failures in `wavry-vr-alvr`'s Windows module.

## Errors (from CI run 21848405504)

### 1. `windows` crate ambiguity (7 occurrences)
- `error[E0659]: windows is ambiguous` — two versions of the `windows` crate in scope via `openxr` 0.21 vs workspace `=0.58.0`

### 2. Unresolved imports from `windows` crate
- `windows::core::ComInterface`
- `windows::Win32::Foundation::E_FAIL`
- `windows::Win32::Graphics::Direct3D11::*`
- `windows::Win32::Graphics::Dxgi::*`
- `windows::Win32::Media::MediaFoundation::*`
- `windows::Win32::System::Com::CoInitializeEx`
- `windows::Win32::System::Com::CoTaskMemFree`
- `windows::Win32::System::Com::CoUninitialize`
- `windows::Win32::System::Com::COINIT_MULTITHREADED`
- `windows::Win32::System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess`

### 3. Missing types/values (cascading from import failures)
- `DXGI_FORMAT_B8G8R8A8_UNORM` (5 occurrences)
- `DXGI_FORMAT_R8G8B8A8_UNORM` (2 occurrences)
- `DXGI_FORMAT_B8G8R8A8_UNORM_SRGB` (2 occurrences)
- `DXGI_FORMAT_R8G8B8A8_UNORM_SRGB` (2 occurrences)
- `IMFTransform` (2 occurrences)
- `ID3D11Device`
- `ID3D11Texture2D` (4 occurrences)
- `IMFActivate`
- `IMFMediaType` (2 occurrences)
- `D3D11_BOX`
- `MFT_REGISTER_TYPE_INFO`
- `MFT_OUTPUT_DATA_BUFFER`
- `SessionCreateInfo` in `xr::d3d`

### 4. Missing functions
- `MFStartup`
- `MFTEnumEx`
- `MFCreateMediaType` (2 occurrences)
- `MFCreateDXGIDeviceManager`
- `MFCreateMemoryBuffer`
- `MFCreateSample`
- `D3D11CreateDevice`

### 5. Missing constants
- `MF_VERSION`, `MFSTARTUP_FULL`
- `MFVideoFormat_AV1`, `MFVideoFormat_HEVC`, `MFVideoFormat_H264`, `MFVideoFormat_RGB32`
- `MFMediaType_Video` (3 occurrences)
- `MF_MT_MAJOR_TYPE`, `MF_MT_SUBTYPE`
- `MF_SA_D3D11_AWARE`
- `MFT_MESSAGE_SET_D3D_MANAGER`
- `MF_E_TRANSFORM_NEED_MORE_INPUT`
- `MFT_CATEGORY_VIDEO_DECODER`
- `MFT_ENUM_FLAG_HARDWARE`, `MFT_ENUM_FLAG_SORTANDFILTER`
- `D3D_DRIVER_TYPE_HARDWARE`, `D3D11_CREATE_DEVICE_BGRA_SUPPORT`, `D3D11_SDK_VERSION`

### 6. Type annotation errors (7 occurrences)
- `error[E0282]: type annotations needed` — cascading from ambiguous `windows` crate

### 7. Type mismatches (3 occurrences)
- `error[E0308]: mismatched types` (2)
- `error[E0271]: type mismatch resolving <D3D11 as Graphics>::Format == i64`

## Fix Strategy

**Option A (Recommended):** Revert `openxr` back to 0.16 and instead patch the specific `LARGE_INTEGER` bug with a `[patch.crates-io]` section or by forking.

**Option B:** Find the exact `openxr` version that uses `windows = "0.58"` and pin to that.

**Option C:** Update the workspace `windows` version to match what `openxr` 0.21 expects, then fix all downstream breakage.
