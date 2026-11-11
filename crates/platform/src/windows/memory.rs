use bediz_core::measurement::Memory;
use windows::Win32::Graphics::Dxgi::{
    DXGI_MEMORY_SEGMENT_GROUP, DXGI_MEMORY_SEGMENT_GROUP_LOCAL,
    DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO, IDXGIAdapter1,
    IDXGIAdapter3,
};
use windows::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2,
};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetCurrentProcess;
use windows::core::Interface;

pub fn available_physical() -> Option<u64> {
    let mut status = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    unsafe { GlobalMemoryStatusEx(&mut status) }
        .ok()
        .map(|()| status.ullAvailPhys)
}

pub fn measure<'a>(adapters: impl Iterator<Item = &'a IDXGIAdapter1>) -> Memory {
    let counters = process_counters();
    let mut memory = Memory {
        working_set_bytes: counters.PrivateWorkingSetSize as u64,
        commit_bytes: counters.PrivateUsage as u64,
        ..Memory::default()
    };
    for adapter in adapters {
        memory.gpu_local_bytes += gpu_usage(adapter, DXGI_MEMORY_SEGMENT_GROUP_LOCAL);
        memory.gpu_shared_bytes += gpu_usage(adapter, DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL);
    }
    memory
}

fn process_counters() -> PROCESS_MEMORY_COUNTERS_EX2 {
    let mut counters = PROCESS_MEMORY_COUNTERS_EX2::default();
    let size = size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32;
    let pointer =
        (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX2).cast::<PROCESS_MEMORY_COUNTERS>();
    match unsafe { GetProcessMemoryInfo(GetCurrentProcess(), pointer, size) } {
        Ok(()) => counters,
        Err(_) => PROCESS_MEMORY_COUNTERS_EX2::default(),
    }
}

fn gpu_usage(adapter: &IDXGIAdapter1, group: DXGI_MEMORY_SEGMENT_GROUP) -> u64 {
    let Ok(adapter) = adapter.cast::<IDXGIAdapter3>() else {
        return 0;
    };
    let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
    match unsafe { adapter.QueryVideoMemoryInfo(0, group, &mut info) } {
        Ok(()) => info.CurrentUsage,
        Err(_) => 0,
    }
}
