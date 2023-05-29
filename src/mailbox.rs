use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};
use std::task::Waker;
use tokio::sync::mpsc::error::TryRecvError;
use triomphe::Arc;
use crate::actor::{Envelope, Signal};



const SYSTEM_CHANNEL_BUF_SIZE: usize = 16; //TODO verify / pick value

pub struct Mailbox<M: Send> {
    /// FIFO for regular messages / signals
    message_receiver: tokio::sync::mpsc::Receiver<Envelope<M>>,

    /// FIFO for high priority 'system' signals that need to bypass regular messages
    system_receiver: tokio::sync::mpsc::Receiver<Signal>,

    suspend_count: AtomicUsize,
}

impl <M: Send> Mailbox<M> {
    pub fn new(buffer_size: usize) -> (tokio::sync::mpsc::Sender<Envelope<M>>, tokio::sync::mpsc::Sender<Signal>, Mailbox<M>) {
        let (message_sender, message_receiver) = tokio::sync::mpsc::channel(buffer_size);
        let (system_sender, system_receiver) = tokio::sync::mpsc::channel(SYSTEM_CHANNEL_BUF_SIZE);

        (
            message_sender,
            system_sender,
            Mailbox {
                message_receiver,
                system_receiver,
                suspend_count: AtomicUsize::new(0),
            },
        )
    }

    pub async fn next(&mut self) -> Option<Envelope<M>> {
        let is_suspended = self.suspend_count.load(Ordering::Acquire) > 0;
        if is_suspended {
            self.system_receiver.recv().await
                .map(|s| s.into())
        }
        else {
            match self.system_receiver.try_recv() {
                Ok(signal) => return Some(signal.into()),
                Err(TryRecvError::Disconnected) => return None, //TODO
                Err(TryRecvError::Empty) => {}
            };

            self.message_receiver.recv().await
        }
    }

    /// Suspends regular message processing for this mailbox, maintaining only 'system' signal
    ///  processing. This function keeps track of repeated calls, and unsuspend() must be called
    ///  for each call to suspend() before regular messages are processed.
    pub fn suspend(&self) {
        self.suspend_count.fetch_add(1, Ordering::Release);
    }

    /// Undo a call to suspend()
    pub fn unsuspend(&self) {
        let prev = self.suspend_count.fetch_sub(1, Ordering::Release);
        assert!(prev > 0, "call to unsuspend() without corresponding call to suspend()");
    }
}
