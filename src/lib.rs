#![feature(core)]
extern crate core;

use std::any::Any;
use std::cell::Cell;
use core::marker::PhantomData;
use std::rc::Rc;
use std::thread;
use std::sync::{Arc, Mutex};

//TODO: This is way inefficient, implement an actual queue
pub struct Queue<T> {
    items: Vec<T>,
}

impl<T> Queue<T> {
    
    pub fn new() -> Self {
        Queue{items: Vec::new()}
    }

    pub fn enqueue(&mut self, item: T) {
        self.items.push(item);
    }

    pub fn dequeue(&mut self) -> Option<T> {
        if self.items.len() > 0 {
            Some(self.items.remove(0))
        } else {
            None
        }
    }
}





trait UntypedMessage: Send + Sync {
    fn as_any<'a>(&'a self) -> &'a Any;
}

/// Because the Any type cannot be sent across threads, we need to wrap the actual message in a
/// struct, send that, and then do the conversion to &Any afterwards
struct Message<T: Send + Sync + Any> {
    m: T
}
impl<T: Send + Sync + Any> UntypedMessage for Message<T> {

    fn as_any<'a>(&'a self) -> &'a Any {
        &self.m as &Any
    }
}



pub trait Actor : Send + Sync {
    fn receive(&self, t: &Any);
}


trait DispatchedActor : Send + Sync {
    fn receive_next(&self);
}

struct LiveActor {
    actor: Box<Actor>,
    mailbox: Mutex<Queue<Box<UntypedMessage>>>,
}

impl LiveActor {

    fn new(actor: Box<Actor>) -> Self {
        LiveActor{actor: actor, mailbox: Mutex::new(Queue::new())}
    }

    fn receive_next(&self) {
        let next = {
            let mut m = self.mailbox.lock().unwrap();
            m.dequeue()
        };
        match next {
            Some(ref t) => {
                self.actor.receive(t.as_any());
            },
            None => {}
        };
    }

    fn send<T: Sync + Send + Any>(&self, message: T) {
        let mut b = self.mailbox.lock().unwrap();
        b.enqueue(Box::new(Message{m: message}));
    }

}


struct StoredActor {
    live: Arc<LiveActor>
}
    

impl DispatchedActor for StoredActor {
    fn receive_next(&self) {
        self.live.receive_next();
    }
}

#[derive(Clone)]
pub struct ActorRef {
    live: Arc<LiveActor>,
}

impl ActorRef {
    pub fn send<T: Send + Sync + Any>(&self, t: T) {
        self.live.send(t);
    }
}


pub struct Dispatcher<'a> {
    actors: Mutex<Vec<Box<DispatchedActor>>>,
    _marker: PhantomData<&'a DispatchedActor>
}


impl<'a> Dispatcher<'a> {

    pub fn new() -> Self {
        Dispatcher{actors: Mutex::new(Vec::new()), _marker: PhantomData}
    }

    pub fn add(&self, actor: Box<Actor>) -> ActorRef {
        let live = Arc::new(LiveActor::new(actor));
        let stored = Box::new(StoredActor{live: live.clone()});

        let mut actors = self.actors.lock().unwrap();
        actors.push(stored as Box<DispatchedActor>);

        ActorRef{live: live}    
    }

    pub fn dispatch(&self) {
        let actors = self.actors.lock().unwrap();
        for actor in actors.iter() {
            actor.receive_next();
        }
    }


}

trait Foo: Send + Sync  {}
impl Foo for i32 {}

#[macro_export]
macro_rules! receive {
    ($ide:ident, $i:ident : $t:ty => $b:block, $($rest:tt)*) => { match $ide.downcast_ref::<$t>() {
        Some($i) => $b,
        None => receive!($ide, $($rest)*)
    }};
    ($ide:ident, $i:ident : $t:ty => $b:block) => { match $ide.downcast_ref::<$t>() {
        Some($i) => $b,
        None => {}
    }}
}

struct Ping(ActorRef, i32);

#[test]
fn test_actor() {
    struct MyActor(i32);
    impl Actor for MyActor {
        fn receive(&self, message: &Any) {
            let &MyActor(i) = self;
            receive!(message,
                p: Ping => {
                    let &Ping(ref sender, ref num) = p;
                    //let q: Ping = p;
                    //let Ping(sender, num) = q;
                    if *num == 500000 {
                        println!("done");
                    } else {
                        sender.send(Ping(sender.clone(), num + 1));
                    }
                }
            );
        }
    }
    let dispatcher = Arc::new(Dispatcher::new());
    /*
    let dispatcher2 = dispatcher.clone();
    let handle = thread::spawn(move || {
        loop {
            //println!("dispatching");
            dispatcher2.dispatch();
            //thread::sleep_ms(2);
        }
    });
    */
    let actor = Box::new(MyActor(3));
    let act = dispatcher.add(actor);

    let actor2 = Box::new(MyActor(2));
    let act2 = dispatcher.add(actor2);

    for i in 0..10 {
        act.send(i);
    }
    act.send(Ping(act2.clone(), 0));
    act2.send(act.clone());

    println!("start");
    let mut m = Mutex::new(Queue::new());
        let mut q = m.lock().unwrap();
    for i in 0..5000000 {
        q.enqueue(act.clone());
        q.dequeue();
    }
    println!("end");


    thread::sleep_ms(1000);

}

