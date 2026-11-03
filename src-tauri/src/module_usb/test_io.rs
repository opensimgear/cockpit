//! Blocking protocol peer for worker tests; no timeout/read polling in the fake.
use super::*;
use crate::transport::duplex::{BlockingReceiver,CancelIo,DeadlineWriter,DuplexParts};
use std::sync::Condvar;

#[derive(Default)]
struct Queue {bytes:VecDeque<u8>,closed:bool,writes:Vec<String>}
#[derive(Default)]
pub(super) struct Pipe {queue:Mutex<Queue>,changed:Condvar}
impl Pipe {
    pub(super) fn send(&self,bytes:&[u8]){
        let mut queue=self.queue.lock().unwrap();if !queue.closed{queue.bytes.extend(bytes);self.changed.notify_all();}
    }
    pub(super) fn wait_command(&self,command:&str,count:usize){
        let until=Instant::now()+Duration::from_secs(2);
        let mut queue=self.queue.lock().unwrap();
        while queue.writes.iter().filter(|line|line.as_str()==command).count()<count{
            let remaining=until.checked_duration_since(Instant::now()).expect("command was not written");
            queue=self.changed.wait_timeout(queue,remaining).unwrap().0;
        }
    }
}
impl CancelIo for Pipe {
    fn cancel(&self){let mut queue=self.queue.lock().unwrap();queue.closed=true;self.changed.notify_all();}
}
struct Rx(Arc<Pipe>);
impl BlockingReceiver for Rx {
    fn receive(&mut self,bytes:&mut[u8])->Result<usize,String>{
        let mut queue=self.0.queue.lock().unwrap();
        while queue.bytes.is_empty()&&!queue.closed{queue=self.0.changed.wait(queue).unwrap();}
        if queue.closed{return Err("cancelled".into());}
        let length=bytes.len().min(13).min(queue.bytes.len());
        for byte in &mut bytes[..length]{*byte=queue.bytes.pop_front().unwrap();}Ok(length)
    }
}
struct Tx {connection:FakeConnection,pipe:Arc<Pipe>}
impl DeadlineWriter for Tx {
    fn write_all(&mut self,bytes:&[u8],deadline:Instant)->Result<(),String>{
        if Instant::now()>=deadline{return Err("test write deadline".into());}
        self.connection.write_all(bytes).map_err(|e|e.to_string())?;
        {let mut queue=self.pipe.queue.lock().unwrap();
            queue.writes.push(String::from_utf8_lossy(bytes).trim().to_owned());self.pipe.changed.notify_all();}
        let output=self.connection.output.drain(..).collect::<Vec<_>>();self.pipe.send(&output);Ok(())
    }
    fn control_lines(&mut self,dtr:bool,rts:bool)->Result<(),String>{
        crate::transport::Connection::set_control_lines(&mut self.connection,dtr,rts)
    }
}
pub(super) fn open(device:Arc<Mutex<Device>>)->DuplexParts{
    let pipe=Arc::new(Pipe::default());
    {let mut state=device.lock().unwrap();state.opens+=1;state.stream_pipe=Some(pipe.clone());}
    pipe.send(format!("{}\r\n",module_console::CONSOLE_BANNER).as_bytes());
    DuplexParts{receiver:Box::new(Rx(pipe.clone())),writer:Box::new(Tx{
        connection:FakeConnection{device,output:VecDeque::new(),input:vec![]},pipe:pipe.clone()}),cancel:pipe}
}
pub(super) fn state(device:&Device,sampled:u32)->Value{
    let config=&device.active;let count=|key:&str|config[key].as_array().unwrap().len();
    let contacts=count("buttons");let encoders=count("encoders");let selectors=count("selectors");
    let axes=config["axes"].as_array().unwrap();let buttons=contacts+encoders*2+selectors*3;
    let maintained=config["buttons"].as_array().unwrap().iter().enumerate()
        .filter(|(_,row)|row[2]==1).map(|(i,_)|i).collect::<Vec<_>>();
    let neutral=axes.iter().map(|row|row[4].as_u64().unwrap()as u16).collect::<Vec<_>>();
    json!({"ok":true,"schemaVersion":1,"stream":device.generation,"head":device.head,"oldest":1,"overflow":0,
        "buttonCount":buttons,"held":vec![false;buttons],"contactButtonCount":contacts,"maintainedButtons":maintained,
        "encoderButtons":(contacts..contacts+encoders*2).collect::<Vec<_>>(),
        "selectorButtons":(0..selectors).map(|i|{let base=contacts+encoders*2+i*3;vec![base,base+1,base+2]}).collect::<Vec<_>>(),
        "axisCount":axes.len(),"axisUsage":axes.iter().map(|row|row[1].as_u64().unwrap()).collect::<Vec<_>>(),
        "axes":neutral,"axisNeutral":neutral,"axisAvailable":vec![false;axes.len()],"sampledAtMs":sampled})
}
