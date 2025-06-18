// event.rs
use std::{
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

use crate::app::TerminalEvent;
use crossterm::event::{self, Event as CrosstermEvent, KeyEvent, MouseEvent};

pub enum Event {
    Tick,
    Input(KeyEvent),
    Mouse(MouseEvent),
    TerminalEvent(TerminalEvent),
}

pub struct EventHandler {
    receiver: Receiver<Event>,
    #[allow(dead_code)]
    event_thread: thread::JoinHandle<()>,
}

impl EventHandler {
    pub fn new(tick_rate: Duration) -> EventHandler {
        let (sender, receiver) = mpsc::channel();
        let event_thread = thread::spawn(move || {
            let mut last_tick = Instant::now();
            loop {
                let timeout = tick_rate
                    .checked_sub(last_tick.elapsed())
                    .unwrap_or_else(|| Duration::from_secs(0));

                // Poll for a crossterm event.
                if event::poll(timeout).expect("Unable to poll for events") {
                    match event::read().expect("Unable to read event") {
                        CrosstermEvent::Key(e) => sender
                            .send(Event::Input(e))
                            .expect("Failed to send key event"),
                        CrosstermEvent::Mouse(e) => sender
                            .send(Event::Mouse(e))
                            .expect("Failed to send mouse event"),
                        CrosstermEvent::Resize(_, _) => sender
                            .send(Event::TerminalEvent(TerminalEvent::Resize))
                            .expect("Failed to send resize event"),
                        _ => {}
                    }
                }

                // If enough time has passed, send a `Tick` event.
                if last_tick.elapsed() >= tick_rate {
                    sender.send(Event::Tick).expect("Failed to send tick event");
                    last_tick = Instant::now();
                }
            }
        });
        EventHandler {
            receiver,
            event_thread,
        }
    }

    pub fn next(&self, timeout: Duration) -> Result<Option<Event>, mpsc::RecvTimeoutError> {
        self.receiver.recv_timeout(timeout).map(Some)
    }
}

impl Event {
    pub fn next(timeout: Duration) -> Result<Option<Event>, Box<dyn std::error::Error>> {
        let handler = EventHandler::new(Duration::from_millis(1000)); // Default tick rate
        Ok(handler.next(timeout)?)
    }
}
