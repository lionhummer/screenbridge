'use strict';
(() => {
  const canvas=document.querySelector('#screen'), status=document.querySelector('#status'), metrics=document.querySelector('#metrics'), ctx=canvas.getContext('2d');
  if(!globalThis.VideoDecoder){status.textContent='WebCodecs requires a supported browser and HTTPS.';return;}
  const ws=new WebSocket(`${location.protocol==='https:'?'wss:':'ws:'}//${location.host}/stream`);ws.binaryType='arraybuffer';
  let decoder,codec,waiting=true,role='viewer',width=0,height=0,buttons=0,displayed=0,bytes=0,resets=0,decodeMs=0,rtt=null,previousFrames=0,previousBytes=0,previousTime=performance.now(),retry;
  const receipts=new Map();
  function fail(message){status.hidden=false;status.textContent=message;if(decoder&&decoder.state!=='closed')decoder.close();ws.close();if(!retry)retry=setTimeout(()=>location.reload(),2000);}
  function configure(next){if(decoder?.state==='configured'&&codec===next)return;decoder?.close();codec=next;waiting=true;receipts.clear();decoder=new VideoDecoder({output(frame){try{const at=receipts.get(frame.timestamp);if(at!==undefined)decodeMs=performance.now()-at;receipts.delete(frame.timestamp);ctx.drawImage(frame,0,0,canvas.width,canvas.height);displayed++;status.hidden=true;}finally{frame.close();}},error(){fail('Decoder failed');}});decoder.configure({codec,optimizeForLatency:true});}
  ws.onmessage=({data})=>{
    if(typeof data==='string'){let m;try{m=JSON.parse(data);}catch{fail('Invalid control message');return;}
      if(m.t==='role'){role=m.role;}else if(m.t==='pong'&&Number.isFinite(m.at)){rtt=performance.now()-m.at;}
      else if(m.t==='init'){if(m.version!==1||!Number.isInteger(m.width)||!Number.isInteger(m.height)||m.width<2||m.height<2||m.width*m.height>16777216||!Number.isInteger(m.sourceWidth)||!Number.isInteger(m.sourceHeight)||m.sourceWidth<m.width||m.sourceHeight<m.height||!/^[a-z0-9]{4}\.[0-9a-f]{6}$/i.test(m.codec)){fail('Invalid stream configuration');return;}width=m.sourceWidth;height=m.sourceHeight;if(canvas.width!==m.width)canvas.width=m.width;if(canvas.height!==m.height)canvas.height=m.height;const firstConfiguration=!decoder;configure(m.codec);if(firstConfiguration)canvas.focus();}return;}
    if(!decoder||data.byteLength<=16)return;const h=new DataView(data);if(h.getUint16(0)!==0x5342||h.getUint8(2)!==1){fail('Protocol mismatch');return;}
    bytes+=data.byteLength;const key=(h.getUint8(3)&1)!==0;
    if(decoder.decodeQueueSize>8){decoder.reset();decoder.configure({codec,optimizeForLatency:true});waiting=true;receipts.clear();resets++;}
    if(waiting&&!key)return;waiting=false;const pts=Math.round(h.getFloat64(8)*1000);if(!Number.isFinite(pts)||pts<0){fail('Invalid timestamp');return;}
    if(receipts.size>=120)receipts.clear();receipts.set(pts,performance.now());try{decoder.decode(new EncodedVideoChunk({type:key?'key':'delta',timestamp:pts,data:data.slice(16)}));}catch{fail('Decode failed');}
  };
  ws.onclose=()=>fail('Disconnected; reconnecting…');ws.onerror=()=>fail('Connection failed');
  function input(message){if(role==='controller'&&ws.readyState===WebSocket.OPEN&&ws.bufferedAmount<65536)ws.send(JSON.stringify(message));}
  function pointer(event,mask){const r=canvas.getBoundingClientRect();if(!r.width||!r.height)return;input({t:'ptr',x:Math.round(Math.max(0,Math.min(width-1,(event.clientX-r.left)*width/r.width))),y:Math.round(Math.max(0,Math.min(height-1,(event.clientY-r.top)*height/r.height))),m:mask});}
  function mask(dom){return(dom&1?1:0)|(dom&4?2:0)|(dom&2?4:0);}
  canvas.addEventListener('mousemove',e=>pointer(e,buttons));canvas.addEventListener('mousedown',e=>{e.preventDefault();canvas.focus();buttons=mask(e.buttons);pointer(e,buttons);});
  window.addEventListener('mouseup',e=>{buttons=mask(e.buttons);pointer(e,buttons);});canvas.addEventListener('contextmenu',e=>e.preventDefault());
  canvas.addEventListener('wheel',e=>{e.preventDefault();const pulse=(e.deltaY<0?8:e.deltaY>0?16:0)|(e.deltaX<0?32:e.deltaX>0?64:0);if(pulse){pointer(e,buttons|pulse);pointer(e,buttons);}},{passive:false});
  const keysyms={Enter:0xff0d,Backspace:0xff08,Tab:0xff09,Escape:0xff1b,Delete:0xffff,Home:0xff50,End:0xff57,PageUp:0xff55,PageDown:0xff56,ArrowLeft:0xff51,ArrowUp:0xff52,ArrowRight:0xff53,ArrowDown:0xff54,Shift:0xffe1,Control:0xffe3,Alt:0xffe9,Meta:0xffeb};
  function key(e,down){let k=keysyms[e.key];if(k===undefined&&/^F([1-9]|1[0-2])$/.test(e.key))k=0xffbd+Number(e.key.slice(1));if(k===undefined&&Array.from(e.key).length===1){const cp=e.key.codePointAt(0);k=cp<256?cp:0x01000000+cp;}if(k!==undefined){e.preventDefault();input({t:'key',k,d:down});}}
  canvas.addEventListener('keydown',e=>key(e,true));canvas.addEventListener('keyup',e=>key(e,false));window.addEventListener('blur',()=>{buttons=0;input({t:'release'});});window.addEventListener('pagehide',()=>{input({t:'release'});ws.close();decoder?.close();});
  setInterval(()=>{const now=performance.now(),seconds=(now-previousTime)/1000;metrics.textContent=`${role} | ${((displayed-previousFrames)/seconds).toFixed(1)} fps | ${((bytes-previousBytes)*8/seconds/1e6).toFixed(2)} Mbps | RTT ${rtt===null?'?':rtt.toFixed(0)} ms | receive-to-draw ${decodeMs.toFixed(1)} ms | resets ${resets}`;previousTime=now;previousFrames=displayed;previousBytes=bytes;if(ws.readyState===WebSocket.OPEN)ws.send(JSON.stringify({t:'ping',at:now}));},2000);
})();
