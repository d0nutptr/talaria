#[cfg(loom)]
mod loom;

#[cfg(shuttle)]
mod shuttle;

#[cfg(not(any(loom, shuttle)))]
mod standard;