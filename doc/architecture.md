## Overview

The purpose of rakka is to allow sending of asynchronous messages to / between actors in a robust and 
reliable way.

The key concepts are as follows:

* An `Actor` is a receiver of *messages*, having identity and (potentially) mutable state. Messages are
    handled in a strictly sequential order, i.e. 'as if' they were handled by sequential calls in a single
    thread. All actors are *typed* in the sense that any given actor can receive only messages of a single
    type (which is typically an enum). Actors have a lifecycle, and they can terminate.
* An `ActorSystem` is the environment in which actors live. There is no technical restriction that prevents
    the existence of several actor systems in a single process at the same time, but there is typically no
    reason to have more than one.
* Actor objects themselves are implementation details of rakka. In application code they are represented
    by `ActorRef` instances, i.e. lightweight and cheaply cloneable handles. Messages are sent to an actor
    through one of the *ActorRef* instances referencing it. Every actor has a unique `ActorId` which is
    automatically assigned. It is largely used for logging and should not usually be of interest to 
    application code. 
* Messages sent to an actor are queued in its `Mailbox` until they are handled by the actor asynchronous. 
    Sending a message never blocks, and it is never delayed by waiting for the handling of a message to
    complete. Mailboxes are however bounded, and sending a message can fail - most notably if the mailbox
    is full, or if the target actor is terminated.
* Every actor has its `ActorContext` which is its API for interacting with the actor system as a whole.
    It is available during message handling and allows message handling code to interact with the actor
    system.
* Every actor has a `Behavior` which wraps an actor's *state* and its *message handling*. An actor's behavior
    is basically the part of an actor that the application cares about, while the rest is largely plumbing
    and infrastructure. 
    TODO there are two styles of using behavior - 'in-place' and 'replacing'
    TODO stack, become
    TODO impl for Fn


supervision
    initial state used on restart

lifecycle hooks

message, signal, envelope
