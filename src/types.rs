// types.rs - type aliasing for sanity

extern crate portmidi as pm;
pub type MidiRes = Result<(), pm::Error>;
