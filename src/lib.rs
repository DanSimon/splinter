#![feature(core)]
extern crate core;

use std::any::Any;
use std::cell::Cell;
use core::marker::PhantomData;
use std::rc::Rc;
use std::thread;
use std::sync::{Arc, Mutex};

trait Message: Any + Send + Sync {}
impl<T: Any + Send + Sync> Message for T {}

trait Actor : Send + Sync {
    fn receive(&self, t: &Message);
}


trait DispatchedActor : Send + Sync {
    fn receiveNext(&self);
}

struct LiveActor {
    actor: Box<Actor>,
    mailbox: Mutex<Option<Box<Message>>>,
}

impl LiveActor {

    fn new(actor: Box<Actor>) -> Self {
        LiveActor{actor: actor, mailbox: Mutex::new(None)}
    }

    fn receiveNext(&self) {
        let mut m = self.mailbox.lock().unwrap();
        *m = match *m {
            Some(ref t) => {
                self.actor.receive(&*t);
                None
            },
            None => None
        }
    }

    fn send(&self, message: Box<Message>) {
        let mut b = self.mailbox.lock().unwrap();
        *b = Some(message);
    }

}


struct StoredActor {
    live: Arc<LiveActor>
}
    

impl DispatchedActor for StoredActor {
    fn receiveNext(&self) {
        self.live.receiveNext();
    }
}

#[derive(Clone)]
struct ActorRef {
    live: Arc<LiveActor>,
}

impl ActorRef {
    fn send(&self, t: Box<Message>) {
        self.live.send(t);
    }
}


struct Dispatcher<'a> {
    actors: Mutex<Vec<Box<DispatchedActor>>>,
    _marker: PhantomData<&'a DispatchedActor>
}


impl<'a> Dispatcher<'a> {

    fn new() -> Self {
        Dispatcher{actors: Mutex::new(Vec::new()), _marker: PhantomData}
    }

    fn add(&self, actor: Box<Actor>) -> ActorRef {
        let live = Arc::new(LiveActor::new(actor));
        let stored = Box::new(StoredActor{live: live.clone()});

        let mut actors = self.actors.lock().unwrap();
        actors.push(stored as Box<DispatchedActor>);

        ActorRef{live: live}    
    }

    fn dispatch(&self) {
        let actors = self.actors.lock().unwrap();
        for actor in actors.iter() {
            actor.receiveNext();
        }
    }


}


#[test]
fn test_actor() {
    struct MyActor(i32);
    impl Actor for MyActor {
        fn receive(&self, message: &Message) {
            let &MyActor(i) = self;
            let a = message as &Any;
            match a.downcast_ref::<i32>() {
                Some(num) => {
                    println!("{} got the number {}", i, num);
                },
                None => {}
            }
        }
    }
    let dispatcher = Arc::new(Dispatcher::new());
    let dispatcher2 = dispatcher.clone();
    let handle = thread::spawn(move || {
        loop {
            //println!("dispatching");
            dispatcher2.dispatch();
            //thread::sleep_ms(2);
        }
    });
    let actor = Box::new(MyActor(3));
    let act = dispatcher.add(actor);

    let actor2 = Box::new(MyActor(2));
    let act2 = dispatcher.add(actor2);

    println!("beginning send");
    act.send(Box::new(34));

    act.send(Box::new(23));
    act2.send(Box::new(99));
    thread::sleep_ms(100);

}

#[test]
fn it_works() {
}
