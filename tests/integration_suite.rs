mod integration;
mod support;

// Count live heap bytes directly so memory assertions don't ride on process RSS,
// which is polluted by allocator pool retention and run-order effects.
#[global_allocator]
static GLOBAL: support::TrackingAllocator = support::TrackingAllocator;
