// prototype.rs

/*
Prototype program - nothing much here
Used as a demo program for testing Portmidi features
*/

use std::{thread, time};
extern crate portmidi as pm;

extern crate instruments as src;
use src::devices::device::*;
use src::types::*;

fn main() -> MidiRes {
    let ctx = pm::PortMidi::new()?;
    let target: &str = "Midi Through Port-0";
    let mut dev = Device::new(&target, &ctx).expect("Failed");

    // do a write                          ?     note vel  ?
    //let _r1 = output_port.write_message([0x90, 35, 101, 4]);
    //let melody: [u8; 8] = [10, 20, 30, 40, 50, 60, 70, 80];
    let melody: [u8; 16] = [
        30, 30, 30, 40, 45, 55, 20, 57, 30, 30, 55, 57, 59, 30, 30, 30,
    ];
    loop {
        for note in melody {
            dev.output.write_message([0x90, note, 127, 1])?;
            thread::sleep(time::Duration::from_millis(100));
        }
    }
}

// end prototype.rs
