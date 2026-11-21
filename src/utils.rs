use std::time::Instant;

/// Set the system master volume to a given percentage (0–100).
/// Uses Windows Core Audio COM API (IAudioEndpointVolume).
pub fn set_volume(percentage: u32) {
    if let Err(e) = unsafe { set_volume_inner(percentage) } {
        console_log!("Error setting volume: {}", e);
    }
}

unsafe fn set_volume_inner(percentage: u32) -> windows::core::Result<()> {
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{
        eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    // Initialize COM — ignore S_FALSE (already initialized)
    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

    let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;

    let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;

    let volume: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None)?;

    let scalar = percentage as f32 / 100.0;
    volume.SetMasterVolumeLevelScalar(scalar, std::ptr::null())?;

    console_log!("Volume successfully set to {}%", percentage);
    Ok(())
}

/// Measure and log execution time of a closure (in minutes).
pub fn time_it<F>(name: &str, f: F)
where
    F: FnOnce(),
{
    let start = Instant::now();
    f();
    let elapsed = start.elapsed();
    let minutes = elapsed.as_secs_f64() / 60.0;
    console_log!("Function '{}' took {:.2} min to execute.", name, minutes);
}
