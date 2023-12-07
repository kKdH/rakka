use std::future::poll_fn;
use std::task::{Context, Poll};
use crate::messages::{Envelope, Signal};

const SYSTEM_CHANNEL_BUF_SIZE: usize = 16; //TODO verify / pick value

pub struct Mailbox<M: Send> {
    /// FIFO for regular messages / signals
    message_receiver: tokio::sync::mpsc::Receiver<M>,

    /// FIFO for high priority 'system' signals that need to bypass regular messages
    system_receiver: tokio::sync::mpsc::Receiver<Signal>,
}

impl <M: Send> Mailbox<M> {
    pub fn new(buffer_size: usize) -> (tokio::sync::mpsc::Sender<M>, tokio::sync::mpsc::Sender<Signal>, Mailbox<M>) {
        let (message_sender, message_receiver) = tokio::sync::mpsc::channel(buffer_size);
        let (system_sender, system_receiver) = tokio::sync::mpsc::channel(SYSTEM_CHANNEL_BUF_SIZE);

        (
            message_sender,
            system_sender,
            Mailbox {
                message_receiver,
                system_receiver,
            },
        )
    }

    pub async fn next(&mut self, system_message_only: bool) -> Option<Envelope<M>> {
        poll_fn(|cx| self.poll_next(system_message_only, cx)).await
    }

    pub fn poll_next(&mut self, system_message_only: bool, cx: &mut Context<'_>) -> Poll<Option<Envelope<M>>> {
        match self.system_receiver.poll_recv(cx) {
            Poll::Ready(r) => Poll::Ready(r.map(|s| s.into())),
            Poll::Pending => {
                if system_message_only {
                    Poll::Pending
                }
                else {
                    match self.message_receiver.poll_recv(cx) {
                        Poll::Ready(m) => Poll::Ready(m.map(|msg| Envelope::Message(msg))),
                        Poll::Pending => Poll::Pending,
                    }
                }
            }
        }
    }
}
