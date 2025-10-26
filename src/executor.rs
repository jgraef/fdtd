use std::{
    ops::{
        Deref,
        DerefMut,
    },
    sync::{
        Arc,
        mpsc,
    },
    thread,
    time::{
        Duration,
        Instant,
    },
};

use parking_lot::{
    RwLock,
    RwLockReadGuard,
    RwLockWriteGuard,
};

use crate::simulation::Simulation;

#[derive(Clone, Debug)]
pub struct Executor {
    state: Arc<RwLock<State>>,
    command_tx: mpsc::Sender<Command>,
}

impl Executor {
    pub fn new(simulation: Simulation) -> Self {
        let state = Arc::new(RwLock::new(State {
            simulation,
            running_state: None,
        }));

        let (command_tx, command_rx) = mpsc::channel();

        let _join_handle = thread::spawn({
            let state = state.clone();
            move || {
                run_reactor(command_rx, state);
            }
        });

        Self { state, command_tx }
    }

    fn send_command(&self, command: Command) {
        self.command_tx.send(command).unwrap();
    }

    pub fn read(&self) -> ReadGuard<'_> {
        ReadGuard {
            guard: self.state.read(),
        }
    }

    pub fn write(&self) -> WriteGuard<'_> {
        WriteGuard {
            guard: self.state.write(),
        }
    }

    pub fn single_step(&self) {
        self.send_command(Command::Step);
    }

    pub fn start(
        &self,
        step_interval: Duration,
        on_update: Option<Box<dyn FnMut(&mut Simulation) + Send + Sync + 'static>>,
    ) {
        self.send_command(Command::Start {
            step_interval,
            on_update,
        });
    }

    pub fn stop(&self) {
        self.send_command(Command::Stop);
    }
}

#[derive(Debug)]
struct State {
    simulation: Simulation,
    running_state: Option<RunningState>,
}

#[derive(derive_more::Debug)]
struct RunningState {
    next_step: Instant,
    step_interval: Duration,
    #[debug(ignore)]
    on_update: Option<Box<dyn FnMut(&mut Simulation) + Send + Sync + 'static>>,
}

enum Command {
    Start {
        step_interval: Duration,
        on_update: Option<Box<dyn FnMut(&mut Simulation) + Send + Sync + 'static>>,
    },
    Stop,
    Step,
}

fn run_reactor(command_rx: mpsc::Receiver<Command>, state: Arc<RwLock<State>>) {
    loop {
        let mut state_guard = state.upgradable_read();

        if let Some(running_state) = &state_guard.running_state {
            let now = Instant::now();
            let recv_timeout = running_state.next_step - now;

            if running_state.next_step <= now {
                state_guard.with_upgraded(|state| {
                    let running_state = state
                        .running_state
                        .as_mut()
                        .expect("running state is None after upgrading the lock");

                    state.simulation.step();
                    if let Some(on_update) = &mut running_state.on_update {
                        on_update(&mut state.simulation);
                    }

                    running_state.next_step = now + running_state.step_interval;
                });
            }

            drop(state_guard);

            match command_rx.recv_timeout(recv_timeout) {
                Ok(command) => {
                    match command {
                        Command::Start {
                            step_interval,
                            on_update,
                        } => {
                            let mut state_guard = state.write();
                            let running_state = state_guard
                                .running_state
                                .as_mut()
                                .expect("runnign state is None after command received");
                            running_state.step_interval = step_interval;
                            running_state.on_update = on_update;
                        }
                        Command::Stop => {
                            let mut state_guard = state.write();
                            state_guard.running_state = None;
                        }
                        Command::Step => {}
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
        else {
            drop(state_guard);

            match command_rx.recv() {
                Ok(command) => {
                    match command {
                        Command::Start {
                            step_interval,
                            on_update,
                        } => {
                            let mut state_guard = state.write();
                            state_guard.running_state = Some(RunningState {
                                next_step: Instant::now(),
                                step_interval,
                                on_update,
                            });
                        }
                        Command::Stop => {}
                        Command::Step => {
                            let mut state_guard = state.write();
                            state_guard.simulation.step();
                        }
                    }
                }
                Err(mpsc::RecvError) => break,
            }
        }
    }
}

#[derive(Debug)]
pub struct ReadGuard<'a> {
    guard: RwLockReadGuard<'a, State>,
}

impl<'a> ReadGuard<'a> {
    pub fn running(&self) -> bool {
        self.guard.running_state.is_some()
    }

    pub fn simulation(&self) -> &Simulation {
        &self.guard.simulation
    }
}

impl<'a> Deref for ReadGuard<'a> {
    type Target = Simulation;

    fn deref(&self) -> &Self::Target {
        &self.guard.simulation
    }
}

pub struct WriteGuard<'a> {
    guard: RwLockWriteGuard<'a, State>,
}

impl<'a> WriteGuard<'a> {
    pub fn simulation(&mut self) -> &mut Simulation {
        &mut self.guard.simulation
    }
}

impl<'a> Deref for WriteGuard<'a> {
    type Target = Simulation;

    fn deref(&self) -> &Self::Target {
        &self.guard.simulation
    }
}

impl<'a> DerefMut for WriteGuard<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.guard.simulation
    }
}
