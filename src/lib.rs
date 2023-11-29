#![doc = include_str!("../README.md")]
#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]

pub use config::{ButtonConfig, Mode};
use embassy_time::{with_timeout, Duration, Timer};

mod config;

/// A generic button that asynchronously detects [`ButtonEvent`]s.
#[derive(Debug, Clone, Copy)]
pub struct Button<P> {
    pin: P,
    state: State,
    count: usize,
    config: ButtonConfig,
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
enum State {
    /// Initial state.
    Unknown,
    /// Debounced press.
    Pressed,
    /// The button has been just released, waiting for more presses in the same sequence, or for
    /// the sequence to end.
    Released,
    /// Fully released state, idle.
    Idle,
    /// Waiting for the button to be released.
    PendingRelease,
}

/// Detected button events
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ButtonEvent {
    /// A sequence of 1 or more short presses.
    ShortPress {
        /// The number of short presses in the sequence.
        count: usize,
    },
    /// A long press. This event is returned directly when the button is held for more than
    /// [`ButtonConfig::long_press`].
    LongPress,
}

impl<P> Button<P>
where
    P: embedded_hal_async::digital::Wait + embedded_hal::digital::InputPin,
{
    /// Creates a new button with the provided config.
    pub const fn new(pin: P, config: ButtonConfig) -> Self {
        Self {
            pin,
            state: State::Unknown,
            count: 0,
            config,
        }
    }

    /// Updates the button and returns the detected event.
    ///
    /// Awaiting this blocks execution of the task until a [`ButtonEvent`] is detected so it should
    /// **not** be called from tasks where blocking for long periods of time is not desireable.
    pub async fn update(&mut self) -> ButtonEvent {
        loop {
            match self.state {
                State::Unknown => {
                    if self.is_pin_pressed() {
                        self.state = State::Pressed;
                    } else {
                        self.state = State::Idle;
                    }
                }

                State::Pressed => {
                    match with_timeout(self.config.long_press, self.wait_for_release()).await {
                        Ok(_) => {
                            // Short press
                            self.state = State::Released;
                        }
                        Err(_) => {
                            // Long press detected
                            self.count = 0;
                            self.state = State::PendingRelease;
                            return ButtonEvent::LongPress;
                        }
                    }
                }

                State::Released => {
                    match with_timeout(self.config.double_click, self.wait_for_press()).await {
                        Ok(_) => {
                            // Continue sequence
                            self.debounce_delay().await;
                            if self.is_pin_pressed() {
                                self.count += 1;
                                self.state = State::Pressed;
                            }
                        }
                        Err(_) => {
                            // Sequece ended
                            let count = self.count;
                            self.count = 0;
                            self.state = State::Idle;
                            return ButtonEvent::ShortPress { count };
                        }
                    };
                }

                State::Idle => {
                    self.wait_for_press().await;
                    self.debounce_delay().await;
                    if self.is_pin_pressed() {
                        self.count = 1;
                        self.state = State::Pressed;
                    }
                }

                State::PendingRelease => {
                    self.wait_for_release().await;
                    self.debounce_delay().await;
                    if self.is_pin_releaded() {
                        self.state = State::Idle;
                    }
                }
            }
        }
    }

    fn is_pin_pressed(&self) -> bool {
        self.pin.is_low().unwrap_or(self.config.mode.is_pulldown()) == self.config.mode.is_pullup()
    }

    fn is_pin_releaded(&self) -> bool {
        !self.is_pin_pressed()
    }

    async fn wait_for_release(&mut self) {
        match self.config.mode {
            Mode::PullUp => self.pin.wait_for_high().await.unwrap_or_default(),
            Mode::PullDown => self.pin.wait_for_low().await.unwrap_or_default(),
        }
    }

    async fn wait_for_press(&mut self) {
        match self.config.mode {
            Mode::PullUp => self.pin.wait_for_low().await.unwrap_or_default(),
            Mode::PullDown => self.pin.wait_for_high().await.unwrap_or_default(),
        }
    }

    async fn debounce_delay(&self) {
        Timer::after(self.config.debounce).await;
    }
}
