use tracing::trace;
use crate::actor::{ActorContext, Envelope};

pub trait Behavior<M: Send + 'static> {
    ///NB: *not* async
    fn receive(&mut self, ctx: &mut ActorContext<M>, envelope: Envelope<M>) -> anyhow::Result<()>;
}

//TODO variations - msg / envelope, with / without ctx, ...
impl <F, M> Behavior<M> for F
    where F: FnMut(&mut ActorContext<M>, M),
          M: Send + 'static,
{
    fn receive(&mut self, ctx: &mut ActorContext<M>, envelope: Envelope<M>) -> anyhow::Result<()> {
        match envelope {
            Envelope::Message(msg) => {
                self(ctx, msg)
            },
            Envelope::Signal(sig) => {
                trace!("ignoring signal {:?} in behavior", sig)
            }
        }

        Ok(())
    }
}
