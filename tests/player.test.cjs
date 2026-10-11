const {test}=require('node:test');
const assert=require('node:assert/strict');
const vm=require('node:vm');
const fs=require('node:fs');
const path=require('node:path');

function player(){
  let clears=0,focuses=0,draws=0,decoders=0,closedFrames=0,decoderResets=0,decoded=0,hold=false,activeDecoder,sequence=0;
  const canvas={_width:300,_height:150,addEventListener(){},focus(){focuses++;},getContext(){return {drawImage(){draws++;}};}};
  for(const property of ['width','height'])Object.defineProperty(canvas,property,{get(){return this['_'+property];},set(value){clears++;this['_'+property]=value;}});
  const status={},metrics={};let socket;
  class WebSocket{static OPEN=1;constructor(){socket=this;this.readyState=1;this.bufferedAmount=0;}send(){}close(){this.readyState=3;}}
  class VideoDecoder{constructor(callbacks){decoders++;activeDecoder=this;this.callbacks=callbacks;this.state='unconfigured';this.decodeQueueSize=0;}configure(){this.state='configured';}close(){this.state='closed';}reset(){decoderResets++;this.decodeQueueSize=0;this.state='unconfigured';}decode(chunk){decoded++;if(hold){this.decodeQueueSize++;return;}this.callbacks.output({timestamp:chunk.timestamp,close(){closedFrames++;}});}}
  vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../web/player.js'),'utf8'),{document:{querySelector(selector){return {'#screen':canvas,'#status':status,'#metrics':metrics}[selector];}},VideoDecoder,WebSocket,EncodedVideoChunk:class{constructor(init){Object.assign(this,init);}},location:{protocol:'https:',host:'fixture.invalid'},performance:{now:()=>10},window:{addEventListener(){}},setInterval(){},setTimeout(){return 1;},DataView,Map,ArrayBuffer});
  return {init(overrides={}){socket.onmessage({data:JSON.stringify({t:'init',version:1,width:1280,height:800,sourceWidth:1280,sourceHeight:800,codec:'avc1.420020',...overrides})});},hold(){hold=true;},queue(n){activeDecoder.decodeQueueSize=n;},frame({key=true,size=1}={}){const data=new ArrayBuffer(16+size);const header=new DataView(data);header.setUint16(0,0x5342);header.setUint8(2,1);header.setUint8(3,key?1:0);header.setFloat64(8,++sequence*33);socket.onmessage({data});},stats(){return {clears,focuses,draws,decoders,closedFrames,decoderResets,decoded};}};
}
test('periodic keyframe metadata preserves the canvas and decoder',()=>{
  const p=player();p.init();p.frame();const first=p.stats();
  for(let i=0;i<5;i++){p.init();p.frame();}
  assert.equal(p.stats().clears,first.clears);
  assert.equal(p.stats().focuses,1);
  assert.equal(p.stats().decoders,1);
  assert.equal(p.stats().draws,6);
  assert.equal(p.stats().closedFrames,6);
});
test('a changed stream size resizes the canvas once',()=>{
  const p=player();p.init();const first=p.stats();
  const size={width:1024,height:768,sourceWidth:1024,sourceHeight:768};p.init(size);
  assert.equal(p.stats().clears,first.clears+2);p.init(size);
  assert.equal(p.stats().clears,first.clears+2);
});

test('transient delta-frame bursts drain without waiting for another keyframe',()=>{
  const p=player();p.init();p.frame();p.hold();p.queue(9);p.frame({key:false});
  assert.equal(p.stats().decoded,2);assert.equal(p.stats().decoderResets,0);
  p.frame();assert.equal(p.stats().decoderResets,1);assert.equal(p.stats().decoded,3);
});
test('hard frame cap resets and rejects dependent frames until a keyframe',()=>{
  const p=player();p.init();p.frame();p.hold();p.queue(30);p.frame({key:false});
  assert.equal(p.stats().decoderResets,1);assert.equal(p.stats().decoded,1);
  p.frame({key:false});assert.equal(p.stats().decoded,1);
  p.frame();assert.equal(p.stats().decoded,2);
});
test('queued encoded bytes are capped independently of frame count',()=>{
  const p=player();p.init();p.frame();p.hold();
  for(let i=0;i<4;i++)p.frame({key:false,size:2*1024*1024});
  assert.equal(p.stats().decoderResets,0);
  p.frame({key:false});assert.equal(p.stats().decoderResets,1);
  assert.equal(p.stats().decoded,5);
});
