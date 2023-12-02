use crate::{MessageQueue, Process, Tracee};
use std::marker::PhantomData;

pub struct Pid;

#[derive(Debug)]
pub enum Error {}

#[derive(Debug)]
pub struct Debugger {
    queue: Queue,

    /// Prevent [`Debugger`] implementing Send.
    _not_send: PhantomData<*mut ()>,
}

impl Process for Debugger {
    fn spawn<P: AsRef<std::path::Path>>(
        _: MessageQueue,
        _: P,
        _: Vec<String>,
    ) -> Result<Self, Error> {
        todo!("spawn");
    }

    fn attach(_: MessageQueue, _: Pid) -> Result<Self, Error> {
        todo!("attach");
    }

    fn run(mut self) -> Result<(), Error> {
        todo!("run");
    }
}

impl Tracee for Debugger {
    fn detach(&mut self) {
        todo!("detach");
    }

    fn kill(&mut self) {
        todo!("kill");
    }

    fn pause(&self) {
        todo!("pause");
    }

    fn kontinue(&mut self) {
        todo!("kontinue");
    }

    fn read_process_memory(&self, _: usize, _: usize) -> Result<Vec<u8>, Error> {
        todo!("read_process_memory");
    }

    fn write_process_memory(&mut self, _: usize, _: &[u8]) -> Result<(), Error> {
        todo!("write_process_memory");
    }
}
