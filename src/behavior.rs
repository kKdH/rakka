use tracing::trace;

use crate::context::ActorContext;
use crate::messages::{Envelope, Signal};

pub trait ActorBehavior<M: Send + 'static> {
    ///NB: *not* async
    fn receive(&mut self, ctx: &mut ActorContext<M>, envelope: Envelope<M>) -> anyhow::Result<()>;

    fn pre_start(&mut self, ctx: &mut ActorContext<M>) -> anyhow::Result<()>;

    /// default implementation: emulate stopping the actor by disposing all children and
    ///  calling `post_stop()`
    fn pre_restart(&mut self, ctx: &mut ActorContext<M>) -> anyhow::Result<()> {

        for child in ctx.children() {
            child.signal(Signal::UnregisterDeathWatcher { subscriber: ctx.myself().as_generic() });
            child.stop();
        }
        ctx.children.clear();

        self.post_stop(ctx)
    }
    /// default implementation: re-initialize the actor instance 'as if' it was freshly started,
    ///  calling `pre_start()`
    fn post_restart(&mut self, ctx: &mut ActorContext<M>) -> anyhow::Result<()> {
        self.pre_start(ctx)
    }
    fn post_stop(&mut self, ctx: &mut ActorContext<M>) -> anyhow::Result<()>;
}

//TODO variations - msg / envelope, with / without ctx, ...
impl <F, M> ActorBehavior<M> for F
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

    fn pre_start(&mut self, ctx: &mut ActorContext<M>) -> anyhow::Result<()> {
        let _ = ctx;
        Ok(())
    }

    fn post_stop(&mut self, ctx: &mut ActorContext<M>) -> anyhow::Result<()> {
        let _ = ctx;
        Ok(())
    }
}
