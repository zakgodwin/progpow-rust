pub mod cpu;

#[cfg(any(feature = "opencl", feature = "cuda"))]
pub mod gpu;

pub use self::cpu::PpCPU;

#[cfg(any(feature = "opencl", feature = "cuda"))]
pub use self::gpu::PpGPU;

