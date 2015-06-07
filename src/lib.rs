#![feature(std_misc)]

use std::any::Any;
use std::collections::{VecDeque, HashMap};
use std::collections::hash_map::RandomState;
use std::thread;
use std::cell::RefCell;
use std::sync::Mutex;
use std::sync::mpsc::{channel, Sender, Receiver, TryRecvError};

pub type ActorId = u32;
type DispatcherId = u16;
type DSender = Sender<DispatcherMessage>;

// We're using thread-local storage to store exactly one copy of each dispatcher's sender.  When
// sending messages to actors, the ActorRef looks up its dispatcher's sender.  We do this instead
// of giving the ActorRef a copy of the sender for 2 reasons:
// 1 - A single u16 takes up less space than a Sender, and we're copying ActorRef's like crazy
// 2 - Cloning Senders is sloooooooow (and they don't implement Copy)
thread_local!(static SENDERS: RefCell<HashMap<DispatcherId, DSender, RandomState>> = RefCell::new(HashMap::new()));


// UntypedMessage trait objects are what are actually sent as an actor message.  This appears to be
// the simplest way to send arbitrary values.
trait UntypedMessage : Send {
    fn as_any<'a>(&'a self) -> &'a Any;
}

// Because the Any type cannot be sent across threads, we need to wrap the actual message in a
// struct, send that, and then do the conversion to &Any afterwards
struct Message<T: Send  + Any> {
    m: T
}
impl<T: Send  + Any> UntypedMessage for Message<T> {

    fn as_any<'a>(&'a self) -> &'a Any {
        &self.m as &Any
    }
}



pub trait Actor: Send  {
    fn receive(&mut self, ctx: Context, t: &Any);

    fn pre_start(&mut self, ctx: Context) {}

    fn post_stop(&mut self, ctx: Context) {}
}

pub struct Context<'a> {
    pub me: &'a ActorRef,
    pub sender: &'a ActorRef
}

impl<'a> Context<'a> {

    pub fn shutdown_system(&self) {
        SENDERS.with(|s| {
            let senders = s.borrow();
            for (_, sender) in senders.iter() {
                sender.send(DispatcherMessage::Shutdown);
            }
        });
    }

    pub fn stop(actor: ActorRef) {
        actor.send_internal(DispatcherMessage::Stop(actor.id));
    }

    //pub fn spawn(child: Actor)
}

struct LiveActor {
    actor: Box<Actor>,
    me: ActorRef,
}

impl LiveActor {

    fn new(actor: Box<Actor>, me: ActorRef) -> Self {
        LiveActor{actor: actor, me:me}
    }

    fn enqueue(&mut self, sender: ActorRef, message: Box<UntypedMessage>) {
        self.actor.receive(Context{sender: &sender, me: &self.me}, message.as_any());
    }

    fn pre_start(&mut self) {
        self.actor.pre_start(Context{sender: &self.me, me: &self.me});
    }
    fn post_stop(&mut self) {
        self.actor.post_stop(Context{sender: &self.me, me: &self.me});
    }

}


#[derive(Clone, Copy, Debug)]
pub struct ActorRef {
    id: ActorId,
    dispatcher_id: DispatcherId,
}

impl ActorRef {

    pub fn id(&self) -> ActorId {
        self.id
    }

    pub fn send<T: Send  + Any>(&self, t: T, from: ActorRef) {
        self.send_internal(DispatcherMessage::ActorMessage(from, self.id, Box::new(Message{m: t})));
    }

    fn send_internal(&self, m: DispatcherMessage) {
        SENDERS.with(|refcell| {
            let senders = refcell.borrow();
            if let Some(channel) = senders.get(&self.dispatcher_id) {
                channel.send(m);
            } else {
                panic!("No Dispatcher");
            }
        });

    }

}


enum DispatcherMessage {
    ActorMessage(ActorRef, ActorId, Box<UntypedMessage>),
    AddActor(ActorRef, Box<Actor>),
    Stop(ActorId),
    AddDispatcher(DispatcherId, Sender<DispatcherMessage>),
    Shutdown,
}

pub struct ActorSystem {
    dispatchers: Vec<DispatcherHandle>,
    next_actor_id: Mutex<ActorId>,
    no_sender: ActorRef
}
impl ActorSystem {

    pub fn new(threads: u16) -> Self {
        if threads == 0 {
            panic!("need at least one thread for actorsystem");
        }
        let mut dispatchers = Vec::new();
        for dispatcher_id in 0..threads {
            let (s,r) = channel();
            let mut dispatcher = Dispatcher::new(dispatcher_id, r);
            let thread = thread::spawn(move || {
                dispatcher.dispatch();
                1 //arbitrary
            });

            let handle = DispatcherHandle{
                id: dispatcher_id,
                sender: s.clone(), //probably not needed
                thread: thread,
            };
            SENDERS.with(|refcell| {
                let mut senders = refcell.borrow_mut();
                senders.insert(dispatcher_id, s)
            });
            dispatchers.push(handle);
        }        
        //all the dispatchers need to add each other (and themselves) to their thread-local-storage
        for source in dispatchers.iter() {
            for target in dispatchers.iter() {
                source.sender.send(DispatcherMessage::AddDispatcher(target.id, target.sender.clone()));
            }
        }
        ActorSystem{
            next_actor_id: Mutex::new(1), 
            no_sender: ActorRef{id: 0, dispatcher_id: 1}, //this should be something else
            dispatchers: dispatchers,
        }
    }

    pub fn add(&self, actor: Box<Actor>) -> ActorRef {
        let mut next_id = self.next_actor_id.lock().unwrap();
        let id = *next_id;
        *next_id += 1;
        //round robin dispatchers for now
        let pos: usize = (id % self.dispatchers.len() as u32) as usize;
        let ref dispatcher = self.dispatchers[pos];
        let aref = ActorRef{id: id, dispatcher_id: dispatcher.id};
        dispatcher.sender.send(DispatcherMessage::AddActor(aref, actor));
        aref
    }

    pub fn join(mut self) {
        while let Some(handle) = self.dispatchers.pop() {
            handle.thread.join();
        }
    }

}

struct DispatcherHandle {
    id: DispatcherId,
    sender: Sender<DispatcherMessage>,
    thread: thread::JoinHandle<i32>,
}

struct Dispatcher {
    id: DispatcherId,
    receiver: Receiver<DispatcherMessage>,
    actors: HashMap<ActorId, LiveActor, RandomState>,
}

// this is a super hacky hack.  Parking a thread seems to be a very expensive operation, so ideally
// we only want to do this after a certain number of busy loops with nothing to do
// Obviously we're going to need something a lot smarter here, probably some kind of fork/join
// work-stealing thing
static BUSY_LOOPS:usize = 1000;

impl Dispatcher {


    fn new(id: DispatcherId, receiver: Receiver<DispatcherMessage>) -> Self {
        Dispatcher{
            id: id,
            receiver: receiver,
            actors: HashMap::new()
        }
    }

    
    fn dispatch(&mut self) {
        let mut loops_till_park = BUSY_LOOPS;
        loop {
            let r: Result<DispatcherMessage, TryRecvError> = if loops_till_park == 0 {
                loops_till_park = BUSY_LOOPS;
                //println!("{} parking", self.id);
                Ok(self.receiver.recv().unwrap())
            } else {
                self.receiver.try_recv()
            };
            if let Ok(message) = r {
                loops_till_park = BUSY_LOOPS;
                match message {
                    DispatcherMessage::ActorMessage(from, id, m) => {
                        if let Some(live) = self.actors.get_mut(&id) {
                            live.enqueue(from, m);
                        }
                    },
                    DispatcherMessage::AddActor(aref, actor) => {
                        let id = aref.id;
                        let mut live = LiveActor::new(actor, aref);
                        live.pre_start();
                        self.actors.insert(id, live);
                    },
                    DispatcherMessage::Stop(id) => {
                        if let Some(mut live) = self.actors.remove(&id) {
                            live.post_stop();
                        }
                    },
                    DispatcherMessage::Shutdown => {
                        for (_, actor) in self.actors.iter_mut() {
                            actor.post_stop();
                        }
                        return;
                    },
                    DispatcherMessage::AddDispatcher(id, sender) => {
                        SENDERS.with(|refcell| {
                            let mut senders = refcell.borrow_mut();
                            senders.insert(id, sender)
                        });
                    }
                };
            } else {
                loops_till_park -= 1;
            }
        }
    }


}

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


#[test]
fn test_actor() {
    struct Ping(ActorRef, i32);
    struct MyActor;
    impl Actor for MyActor {
        fn receive(&mut self, ctx: Context, message: &Any) {
            receive!(message,
                num: i32 => {
                    if *num == 50 {
                        println!("done");
                        ctx.shutdown_system();
                    } else {
                        ctx.sender.send(num + 1, *ctx.me);
                    }
                }
            );
        }
    }
    let system = ActorSystem::new(4);
    
    let actor = Box::new(MyActor);
    let act = system.add(actor);

    let actor2 = Box::new(MyActor);
    let act2 = system.add(actor2);

    act.send(0, act2);

    system.join();

}

