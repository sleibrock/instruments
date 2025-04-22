// lparp.rs - an arpeggiator for the Novation Launchpad (mk1 series)

/*
Self-explanatory Arpeggiator for the Novation Launchpad
Works by storing 32 columns with the ability for users to interact
with each button on the device. The program will send MIDI OUT
notes to PortMIDI for other programs to pick it up.

 * 4 buttons to control the view of the 32-column array
 * play/pause buttons to stop or start
 * major/minor button to swap harmonic scales
 * quit button
 * octave control on the right-most column
 * 64 buttons to allow users to select 0-7 on each column
 * bottom-row will set the column to 0
 * bottom-row is also lit up as a "tracker"

Most of the functionality here relies on the use of "MidiRes",
a special Result<(), pm::Error> type alias simply because every
read and write from a device can potentially fail for random reasons.
Using the bubble '?' operator alleviates some pains, but mostly
anything that involves sending or receiving MIDI information can
generate a Result<T,E> type as a result.

TODOs (4/22/2025):
 * documentation
 * split code up into reusable components for future use

*/

use std::thread;
use std::time::{Duration, Instant};

extern crate portmidi as pm;

extern crate instruments as src;
use src::devices::device::*;
use src::types::*;

/// A generic Job container shim to be stored in the scheduler
#[derive(Debug)]
pub struct Job<T> {
    ct: usize,
    mt: usize,
    msg: T,
}

/// A Scheduler layout. Contains tick rate, tick duration, timing
/// and the jobs/queue system.
pub struct Scheduler<T> {
    tick_duration: Duration,
    last_time: Instant,
    jobs: Vec<Job<T>>,
    queue: Vec<T>,
}

/// Scheduler implementation. The item to be used must implement Copy
/// For debugging, add `+ std::fmt::Debug`
impl<T: Copy> Scheduler<T> {
    /// Create a new scheduler with job and queue capacities at 100
    pub fn new() -> Scheduler<T> {
        let jobs = Vec::with_capacity(100);
        let queue = Vec::with_capacity(100);
        Scheduler {
            tick_duration: Duration::new(0, 0),
            last_time: Instant::now(),
            jobs: jobs,
            queue: queue,
        }
    }

    /// Check if the queue has events waiting
    pub fn has_events(&self) -> bool {
        self.queue.len() > 0
    }

    /// Clear the job queue
    pub fn clear_queue(&mut self) {
        // delete all items from queue
        self.queue.clear();
    }

    /// Schedule a job to be executed every N ticks
    pub fn interval(&mut self, tick_amt: usize, msg: T) {
        self.jobs.push(Job {
            ct: 0,
            mt: tick_amt,
            msg: msg,
        })
    }

    /// Calculate a schedule rate based on BPM against microseconds
    /// Start with a minute (in us), divide by ticks x BPM
    pub fn set_rate(&mut self, bpm: i32, num_ticks: i32) {
        let ms = 60000000.0 / (bpm * num_ticks) as f64;
        self.tick_duration = Duration::from_micros(ms as u64);
    }

    /// Update will increase the ticks by one
    /// In order to make sure we are sleeping the thread consistently,
    /// we need to calculate our current timestamps to ensure
    /// we can wait a correct amount of time. To do this we calculate
    /// a delta and sleep for the delta, which will keep us in lockstep
    /// with our target BPM, to ensure all jobs are executed
    /// correctly with their respective time measures.
    pub fn update(&mut self) {
        for job in &mut self.jobs {
            job.ct += 1;
            if job.ct == job.mt {
                job.ct = 0;
                self.queue.push(job.msg);
            }
        }
        // trigger a thread sleep HERE
        let new_time = Instant::now();
        let elapsed = new_time.duration_since(self.last_time);
        let delta = self.tick_duration - elapsed;
        thread::sleep(delta);
        self.last_time = Instant::now();
        // end sleep calculation
    }
}

pub type MidiVal = u8;
pub type BtnArr = [u8; 4];

// heptatonic scales only (7 notes per octave)
#[derive(Debug, Copy, Clone)]
pub enum Scale {
    Major,
    Minor,
}

#[derive(Debug, Copy, Clone)]
pub enum Msg {
    CheckInputs,
    UpdateState,
    FlushNotes,
    Quit,
}

// MIDI message type constants
// I often forget
const MIDI: MidiVal = 0xB0;
const NOTE: MidiVal = 0x90;

// Major: C D E F G A B
// Minor: C D Ef F G Af Bf
const MAJOR_SCALE: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
const MINOR_SCALE: [u8; 7] = [0, 2, 3, 5, 7, 8, 10];

/// Convert a MIDI note and a Scale to a scale-based MIDI message
/// Uses LUTs to convert to either Major or Minor scale
fn calc_note(note: MidiVal, scale: &Scale) -> Option<MidiVal> {
    match (note, scale) {
        (1..7, Scale::Major) => Some(MAJOR_SCALE[note as usize]),
        (1..7, Scale::Minor) => Some(MINOR_SCALE[note as usize]),
        _ => None,
    }
}

/// Converts a MIDI message from 0..127 to (x, y)
/// where (x,y) correspond to the MIDI device output
/// Returns None when MIDI value is out of range
///
/// find_lp_xy(50) -> Some((3, 5))
/// find_lp_xy(200) -> None
fn find_lp_xy(x: MidiVal) -> Option<(u8, u8)> {
    let nx = match x >= 16 {
        true => x % 16,
        _ => x,
    };
    match nx < 9 {
        true => Some((nx, x / 16)),
        _ => None,
    }
}

/// Calculate the LED color on the Launchpad
/// Launchpad only has two color options for LEDs, Red and Green,
/// each with 3 levels of brightness
fn led_color(red: u8, green: u8) -> u8 {
    match (red, green) {
        (0..=3, 0..=3) => 12 | red | (16 * green),
        _ => 127,
    }
}

// Column state for the physical device
// Stores it's value to indicate it's position
// and it's MIDI note value to easily unset the previous LED
// val: a value between 0 and 7
// note: arbitrarily any value between 0-255, preferrably 0-127
#[derive(Debug, Copy, Clone)]
pub struct ArpCol {
    pub val: u8,
    pub note: u8,
}

impl ArpCol {
    fn new() -> ArpCol {
        ArpCol { val: 0, note: 0 }
    }
}

/// The tracker is the visual LED to indicate where we
/// are in the arpeggiator. Keeps track of the index
/// and the last button we were on. Only displays
/// if we're on a matching buffer.
pub struct Tracker {
    pub index: u8,
    pub btn: BtnArr,
}

impl Tracker {
    fn new() -> Tracker {
        Tracker {
            index: 0,
            btn: [NOTE, 112, 127, 0],
        }
    }

    fn in_range(&self, buffer_index: u8) -> bool {
        let bmin = buffer_index*8;
        return (bmin <= self.index) && (self.index <= (bmin+7));
    }

    fn update(&mut self) {
        self.index += 1;
        if self.index == 32 {
            self.index = 0;
        }
    }

    fn move_right(&mut self) {
        self.btn[1] += 1;
        if self.btn[1] == 120 {
            self.btn[1] = 112;
        }
    }
}

/// Arpeggiator struct layout
/// Requires a lifetime for Portmidi device connections
pub struct Arp<'a> {
    pub midi_out: Device<'a>,
    pub grid_io: Device<'a>,
    pub running: bool,
    pub playing: bool,
    pub scheduler: Scheduler<Msg>,
    pub index: usize,
    pub buffer_index: u8,
    pub buffer: [ArpCol; 32],
    pub buffer_btn: BtnArr,
    pub pp_btn: BtnArr,
    pub scale: Scale,
    pub scale_btn: BtnArr,
    pub octave: u8,
    pub octave_btn: BtnArr,
    pub bpm: u8,
    pub tracker: Tracker,
}

impl Arp<'_> {
    fn new<'a>(midi_out: Device<'a>, grid_io: Device<'a>) -> Arp<'a> {
        let buffer_btn = [MIDI, 104, 127, 0];
        let pp_btn = [MIDI, 108, led_color(3, 0), 0];
        let scale_btn = [MIDI, 110, led_color(1, 3), 0];
        let octave_btn = [NOTE, 72, 127, 0];
        Arp {
            midi_out: midi_out,
            grid_io: grid_io,
            running: true,
            playing: false,
            scheduler: Scheduler::new(),
            index: 0,
            buffer_index: 0,
            buffer: [ArpCol::new(); 32],
            buffer_btn: buffer_btn,
            pp_btn: pp_btn,
            scale: Scale::Major,
            scale_btn: scale_btn,
            octave: 5,
            octave_btn: octave_btn,
            bpm: 120,
            tracker: Tracker::new(),
        }
    }

    /// Sets running to `false` to shut the app loop off
    fn quit(&mut self) -> MidiRes {
        println!("Quitting program");
        self.running = false;
        Ok(())
    }

    /// Checks if the device has any inputs
    /// A list of events is scanned from the serial device
    /// and fed in, with each message corresponding to an event
    /// on the MIDI bus. For this device, there are two corresponding
    /// status messages.
    /// 176 => MIDI general message (pd -> midiin)
    /// 144 => MIDI note message (pd -> notein)
    /// Functionally we only care about an event when velocity=127
    fn check_inputs(&mut self) -> MidiRes {
        if let Ok(Some(evts)) = self.grid_io.input.read_n(1024) {
            for e in evts {
                let status = e.message.status;
                let note = e.message.data1;
                let vel = e.message.data2;

                if vel == 0 {
                    return Ok(());
                }
                match status {
                    MIDI => self.top_row_dispatch(note)?,
                    NOTE => self.grid_button_dispatch(note)?,
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// Dispatch logic for top-row MIDI messages
    fn top_row_dispatch(&mut self, note: MidiVal) -> MidiRes {
        if note < 104 {
            return Ok(());
        }
        let idx = note - 104;
        match idx {
            0..=3 => {
                // if the target buffer is different than current,
                // reflash the entire UI and change the buffer index
                // mutate the buffer highlighted button as well
                if idx != self.buffer_index {
                    self.buffer_index = idx;
                    self.buffer_btn[1] = note;
                    self.render_ui()
                } else {
                    Ok(())
                }
            }
            4 => self.pause(),
            5 => self.play(),
            6 => self.invert_scale(),
            7 => self.quit(),
            _ => { Ok(()) }
        }
    }

    /// Dispatch for grid-based MIDI messages
    fn grid_button_dispatch(&mut self, note: MidiVal) -> MidiRes {
        if let Some((x, y)) = find_lp_xy(note) {
            if x == 8 {
                self.grid_io.output.write_message([
                    NOTE, self.octave_btn[1], 0, 0
                ])?;
                self.octave = 7 - y;
                self.octave_btn[1] = note;
                self.grid_io.output.write_message(self.octave_btn)?;
                return Ok(());
            }
            let offset = ((self.buffer_index*8) + x) as usize;
            let new_val = 7 - y; // inverting the value

            // grab a reference to the column
            let column = &mut self.buffer[offset];
            if column.val != new_val {
                // turn off old LED if there was a non-zero value
                if column.val != 0 {
                    self.grid_io.output.write_message([
                        NOTE, column.note, 0, 0
                    ])?;
                }

                // and turning on the new LED
                if new_val != 0 {
                    self.grid_io.output.write_message([
                        NOTE, note, 127, 0
                    ])?;
                }
                column.val = new_val;
                column.note = note;
            }
        }
        Ok(())
    }

    /// Activate the playing mode and toggle the playing LED
    /// while also deactivating the paused LED
    fn play(&mut self) -> MidiRes {
        if !self.playing {
            self.playing = true;
            self.grid_io.write(176, 108, 0, 0);
            self.pp_btn[1] = 109;
            self.pp_btn[2] = led_color(0, 3);
            self.grid_io.output.write_message(self.pp_btn)?;
        }
        Ok(())
    }

    /// Inverse action of .play()
    fn pause(&mut self) -> MidiRes {
        if self.playing {
            self.playing = false;
            self.grid_io.write(176, 109, 0, 0);
            self.pp_btn[1] = 108;
            self.pp_btn[2] = led_color(3, 0);
            self.grid_io.output.write_message(self.pp_btn)?;
        }
        Ok(())
    }

    /// Invert the current scale and change the active LED to reflect it
    fn invert_scale(&mut self) -> MidiRes {
        match self.scale {
            Scale::Major => {
                self.scale = Scale::Minor;
                self.scale_btn[2] = led_color(3, 1);
            },
            _ => {
                self.scale = Scale::Major;
                self.scale_btn[2] = led_color(1, 3);
            }
        }
        self.grid_io.output.write_message(self.scale_btn)
    }

    /// Update all components that rely on a note tick
    fn update_state(&mut self) -> MidiRes {
        // bump the note index counter
        if self.playing {
            self.index += 1;
            if self.index == 32 {
                self.index = 0;
            }
        }

        // turn off the tracker's previous LED
        // do this before we "move" the button
        self.grid_io.output.write_message([
            NOTE, self.tracker.btn[1], 0, 0
        ])?;
        
        self.tracker.update();
        self.tracker.move_right();
        
        // turn on the tracker's LED if it's "on screen"
        if self.tracker.in_range(self.buffer_index) {
            self.grid_io.output.write_message(
                self.tracker.btn
            )?;
        }

        Ok(())
    }

    /// Send note messages from the current state index
    /// Only send messages if a column is active
    fn flush_notes(&mut self) -> MidiRes {
        let col = &self.buffer[self.index];
        if col.val > 0 {
            if let Some(base_note) = calc_note(col.val, &self.scale) {
                self.midi_out.output.write_message([
                    NOTE, base_note+(self.octave*12), 127, 1
                ])?;
            }
        }
        Ok(())
    }

    /// Clears the board of all LED values
    fn clear_board(&mut self) -> MidiRes {
        self.grid_io.output.write_message([MIDI, 0, 0, 0])
    }

    /// Main function to re-draw every element onto the device.
    /// Clears the full thing and sends out all UI LED messages.
    fn render_ui(&mut self) -> MidiRes {
        // clear board for a full wipe
        self.clear_board()?;

        // draw UI elements
        self.grid_io.output.write_message(self.buffer_btn)?;
        self.grid_io.output.write_message(self.pp_btn)?;
        self.grid_io.output.write_message(self.scale_btn)?;
        self.grid_io.output.write_message(self.octave_btn)?;

        // draw tracker if it's on screen
        // note: this part works
        if self.tracker.in_range(self.buffer_index) {
            self.grid_io.output.write_message(self.tracker.btn)?;
        }
        
        // render all cells
        for c in 0..8 {
            let index = ((self.buffer_index*8) + c) as usize;
            let col = &self.buffer[index];
            if col.val > 0 {
                self.grid_io.output.write_message([0x90, col.note, 127, 0])?;
            }
        }
        Ok(())
    }

    /// Wrapper run function to loop both update and schedule update
    fn run(&mut self) -> MidiRes {
        while self.running {
            self.update()?;
            self.scheduler.update();
        }
        Ok(())
    }

    /// Called once per cycle to check if the scheduler has
    /// any messages to process. Since it involves mutation
    /// of the original &self, we iterate by indexing instead
    /// of hard referencing an iterator. Clears the queue
    /// after processing all messages.
    /// Take note that certain events should only be processed
    /// if "playing" is set to true.
    fn update(&mut self) -> MidiRes {
        if self.scheduler.has_events() {
            let mut i = 0;
            while i < self.scheduler.queue.len() {
                match (self.scheduler.queue[i], self.playing) {
                    (Msg::Quit, _) => self.quit()?,
                    (Msg::CheckInputs, _) => self.check_inputs()?,
                    (Msg::UpdateState, true) => self.update_state()?,
                    (Msg::FlushNotes, true) => self.flush_notes()?,
                    _ => {},
                }
                i += 1;
            }
            self.scheduler.clear_queue();
        }
        Ok(())
    }
}

/// Main function. Create PortMidi context, create Arpeggiator,
/// run application loop, then close out.
fn main() -> MidiRes {
    let ctx = pm::PortMidi::new()?;
    let target: &str = "Midi Through Port-0";
    let dev = Device::new(&target, &ctx).expect("Failed");

    let lpname: &str = "Launchpad MIDI 1";
    let lp = Device::new(&lpname, &ctx).expect("Failed");

    let mut arp = Arp::new(dev, lp);

    // (1s / BPM) / NTICKS = tick duration 
    // 60 / 120 = 0.5 / 64 = 0.007
    arp.scheduler.set_rate(120, 64);
    arp.scheduler.interval(4, Msg::CheckInputs);
    arp.scheduler.interval(32, Msg::UpdateState);
    arp.scheduler.interval(32, Msg::FlushNotes);

    // 1 = every tick, or 256th note
    // 2 = 128th
    // 4 = 64th
    // 8 = 32nd
    // 16 = sixteenth
    // 32 = eigth
    // 64 = quarter note (bass drum)
    // 128 = half note (snare drum)
    // 256 = full note (two "beats")

    println!("Beginning program");
    let before = Instant::now();

    arp.clear_board()?;
    arp.render_ui()?;
    arp.run()?;
    arp.clear_board()?;

    let after = before.elapsed();
    println!("Program end. Time passed: {:?}", after.as_secs());
    Ok(())
}

// end lparp.rs
