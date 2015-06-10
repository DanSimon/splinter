# Splinter

Splinter is an actor library for Rust.  It aims to provide a fast, safe, and intuitive implementation of Erlang-style actors.

**WARNING: This project is in the very early stages of development and is very unstable!!**

An actor is an object with an inbox that can receive messages.  Messages are
sent to the actor asynchronously and the actor dequeues and processes messages
in its inbox one at a time.  Actors are non-blocking, meaning that when an
actor's inbox is empty and it's not doing anything, no threads are blocked.
Thus actors provide a nice way to acheive lightweight concurrency.

This project is heavily inspired by the Scala actor library
[Akka](http://akka.io).

Features:
  * Actors are untyped
  * Actors are lightweight (approx 10 million per GB of memory)
  * Millions of actors can share a single thread



## Examples

In this example two actors send increasingly larger integers to each other
until one hits 1 million.

```rust
struct PingPong;
impl Actor for PingPong {
    fn receive(&mut self, ctx: Context, message: &Any) {
        receive!(message,
            num: i32 => {
                if *num == 1000000 {
                    println!("done");
                    ctx.shutdown_system();
                } else {
                    ctx.sender.send(num + 1, *ctx.me);
                }
            }
        );
    }
}

fn ping_pong() {
    //create an ActorSystem with 2 threads
    let system = ActorSystem::new(2);

    let act: ActorRef  = system.add(Box::new(PingPong));
    let act2: ActorRef = system.add(Box::new(PingPong));

    println!("begin");
    //send 0 to act with act2 as the sender
    act.send(0, act2);

    system.join();

}
```

Here's another example that creates a ton of actors and sends a message through all of them:

```rust
struct Forwarder{next: Option<ActorRef>}
impl Actor for Forwarder {
    fn receive(&mut self, ctx: Context, message: &Any) {
        receive!(message,
            foo: Foo => {
                if let Some(next) = self.next {
                    next.send(Foo, *ctx.me);
                } else {
                    println!("done");
                    ctx.shutdown_system();
                }
            }
        );
    }
}
fn forward() {
    let system = ActorSystem::new(4);
    let mut n = system.add(Box::new(Forwarder{next: None}));
    for i in 0..1000000 {
        n = system.add(Box::new(Forwarder{next: Some(n)}));
    }
    println!("starting");
    n.send(Foo, n);
    system.join();
}
```

## Notes

This whole thing is pretty experimental.  It certainly warrents to ask whether
the Actor model even makes sense to implement in Rust, as some of the benefits
of actors (safely sharing data, writing fault-tolerant code) are handled much
better by Rust than other languages.

Sending a message to an actor from any thread besides the main thread or
threads created by the ActorSystem won't work right now.  This is because
thread-local storage is used to hold the channel sender for the actor (this is
done because cloning Senders is very slow)

The name Splinter is inspired by Matthew Mather's [The Atopia
Chronicles](http://amzn.com/B00DUK1RKY), where people can create splinters of
their consciousness to multitask.  Good read, check it out.

### Performance

The message dispatcher right now is extremely naive.  Actors are basically
pinned to threads, so it's possible to get bottlenecked on one thread, though
in practice I've found it actually works pretty well.  Two actors colocated on
the same thread can send messages to each other at a very high rate (this is
because the underlying channel never blocks or busy-waits).


### Upcoming Improvements

* Fix the dispatcher
* Add the ability for actors to create children actors
* "become" semantics
* Actor supervision, probably have `receive` return a Result since Rust doesn't have exceptions
* Better lifecycle management, deathwatches
