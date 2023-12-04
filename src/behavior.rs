use tracing::trace;

use crate::context::ActorContext;
use crate::messages::{Envelope, Signal};
use crate::refs::{ActorRef, SignalSender};




pub struct ForwardingBehavior<A: Send + 'static, B: Send + 'static> {
    target: ActorRef<B>,
    transformation: Box<dyn Fn(A) -> B>,
}
impl <A: Send + 'static, B: Send + 'static> ForwardingBehavior<A, B> {
    pub fn new(target: ActorRef<B>, transformation: impl Fn(A) -> B + Send + 'static) -> ForwardingBehavior<A, B> {
        ForwardingBehavior {
            target,
            transformation: Box::new(transformation),
        }
    }
}
impl <A: Send + 'static, B: Send + 'static> ActorBehavior<A> for ForwardingBehavior<A, B> {
    fn receive(&mut self, _ctx: &mut ActorContext<A>, envelope: Envelope<A>) -> anyhow::Result<()> {
        match envelope {
            Envelope::Message(msg) => {
                self.target.send(self.transformation.as_ref()(msg));
            }
            Envelope::Signal(signal) => {
                self.target.send_signal(signal);
            }
        }
        Ok(())
    }
}



pub trait ActorBehavior<M: Send + 'static> {
    ///NB: *not* async
    fn receive(&mut self, ctx: &mut ActorContext<M>, envelope: Envelope<M>) -> anyhow::Result<()>;

    fn pre_start(&mut self, ctx: &mut ActorContext<M>) -> anyhow::Result<()> {
        let _ = ctx;
        Ok(())
    }

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
    fn post_stop(&mut self, ctx: &mut ActorContext<M>) -> anyhow::Result<()> {
        let _ = ctx;
        Ok(())
    }
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
}
