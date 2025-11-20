use std::{
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

use crate::fdtd::ErasedSimulation;

#[derive(Clone, Debug)]
pub struct Executor {
    state: Arc<RwLock<State>>,
    command_tx: mpsc::Sender<Command>,
}

impl Executor {
    pub fn new(simulation: Box<dyn ErasedSimulation>, step_interval: Duration) -> Self {
        let state = Arc::new(RwLock::new(State {
            simulation,
            running_state: None,
            step_interval,
            step_time: Default::default(),
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
        on_update: Option<Box<dyn FnMut(&mut dyn ErasedSimulation) + Send + Sync + 'static>>,
    ) {
        self.send_command(Command::Start { on_update });
    }

    pub fn stop(&self) {
        self.send_command(Command::Stop);
    }

    pub fn set_step_interval(&self, step_interval: Duration) {
        let mut guard = self.state.write();
        guard.step_interval = step_interval;
    }
}

#[derive(Debug)]
struct State {
    simulation: Box<dyn ErasedSimulation>,
    running_state: Option<RunningState>,
    step_interval: Duration,
    step_time: Duration,
}

#[derive(derive_more::Debug)]
struct RunningState {
    next_step: Instant,
    #[debug(ignore)]
    on_update: Option<Box<dyn FnMut(&mut dyn ErasedSimulation) + Send + Sync + 'static>>,
}

enum Command {
    Start {
        on_update: Option<Box<dyn FnMut(&mut dyn ErasedSimulation) + Send + Sync + 'static>>,
    },
    Stop,
    Step,
}

fn run_reactor(command_rx: mpsc::Receiver<Command>, state: Arc<RwLock<State>>) {
    loop {
        let mut state_guard = state.upgradable_read();

        if let Some(running_state) = &state_guard.running_state {
            let now = Instant::now();
            let mut next_step = running_state.next_step;

            if now >= next_step {
                state_guard.with_upgraded(|state| {
                    let running_state = state
                        .running_state
                        .as_mut()
                        .expect("running state is None after upgrading the lock");

                    let t = Instant::now();
                    state.simulation.update();
                    state.step_time = t.elapsed();

                    if let Some(on_update) = &mut running_state.on_update {
                        on_update(&mut *state.simulation);
                    }

                    next_step = now + state.step_interval;
                    running_state.next_step = next_step;
                });
            }

            drop(state_guard);

            let recv_timeout = next_step - Instant::now();
            match command_rx.recv_timeout(recv_timeout) {
                Ok(command) => {
                    match command {
                        Command::Start { on_update } => {
                            let mut state_guard = state.write();
                            let running_state = state_guard
                                .running_state
                                .as_mut()
                                .expect("runnign state is None after command received");
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
                        Command::Start { on_update } => {
                            let mut state_guard = state.write();
                            state_guard.running_state = Some(RunningState {
                                next_step: Instant::now(),
                                on_update,
                            });
                        }
                        Command::Stop => {}
                        Command::Step => {
                            let mut state = state.write();
                            let t = Instant::now();
                            state.simulation.update();
                            state.step_time = t.elapsed();
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

    pub fn simulation(&self) -> &dyn ErasedSimulation {
        &*self.guard.simulation
    }

    pub fn step_time(&self) -> Duration {
        self.guard.step_time
    }
}

pub struct WriteGuard<'a> {
    guard: RwLockWriteGuard<'a, State>,
}

impl<'a> WriteGuard<'a> {
    pub fn simulation(&mut self) -> &mut dyn ErasedSimulation {
        &mut *self.guard.simulation
    }
}
