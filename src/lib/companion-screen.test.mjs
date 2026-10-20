import assert from 'node:assert/strict';
import test from 'node:test';
import { knownPanelScreenChoices, panelScreenSizeReason } from './companion-screen.ts';

test('new panel screen choices use reported sizes for every companion kind, including offline companions', () => {
  const state={host:{clients:[
    {companionId:'browser',name:'Browser screen',kind:'browser',connected:true,screen:{width:1920,height:1080}},
    {companionId:'tablet',name:'Tablet',kind:'native',connected:false,screen:{width:768,height:1024}},
    {companionId:'esp',name:'ESP32 screen',kind:'esp32',screen:{width:480,height:320}},
    {companionId:'unnamed',name:' ',screen:{width:320,height:8192}},
  ]}};
  const before=structuredClone(state);
  assert.deepEqual(knownPanelScreenChoices(state),[
    {id:'browser',name:'Browser screen',width:1920,height:1080},
    {id:'tablet',name:'Tablet',width:768,height:1024},
    {id:'esp',name:'ESP32 screen',width:480,height:320},
    {id:'unnamed',name:'unnamed',width:320,height:8192},
  ]);
  assert.deepEqual(state,before);
});

test('new panel screen choices use known ESP32 board sizes from USB and saved firmware', () => {
  const state={
    host:{clients:[
      {companionId:'usb',name:' USB screen ',kind:'esp32',connected:false},
      {companionId:'saved',name:'Saved screen',kind:'esp32',connected:false},
      {companionId:'reported',name:'Reported screen',kind:'esp32',screen:{width:1024,height:600}},
    ]},
    usb:{devices:[{connected:false,device:{companionId:'usb',boardProfile:'sunton-3248s035r'}}]},
    firmware:{saved:{profile:'waveshare-lcd7-s3',version:'test'},reported:{profile:'sunton-3248s035r',version:'test'}},
  };
  assert.deepEqual(knownPanelScreenChoices(state),[
    {id:'usb',name:'USB screen',width:480,height:320},
    {id:'saved',name:'Saved screen',width:800,height:480},
    {id:'reported',name:'Reported screen',width:1024,height:600},
  ]);
});

test('new panel screen choices never infer unknown screens from assigned panels or arbitrary firmware profiles', () => {
  const state={
    host:{clients:[
      {companionId:'unknown-browser',kind:'browser'},
      {companionId:'unknown-esp',kind:'esp32'},
      {companionId:'not-esp',kind:'browser'},
    ],instances:[{companionId:'unknown-browser',width:1920,height:1080}]},
    firmware:{'unknown-esp':{profile:'unknown-board',version:'test'},'not-esp':{profile:'sunton-3248s035r',version:'test'}},
  };
  assert.deepEqual(knownPanelScreenChoices(state),[]);
  assert.deepEqual(knownPanelScreenChoices({}),[]);
});

test('new panel screen choices omit invalid dimensions and identities and deduplicate valid companions', () => {
  const state={host:{clients:[
    {companionId:'string-size',screen:{width:'480',height:320}},
    {companionId:'too-small',screen:{width:319,height:320}},
    {companionId:'too-large',screen:{width:320,height:8193}},
    {companionId:'fraction',screen:{width:480.5,height:320}},
    {companionId:'infinite',screen:{width:Infinity,height:320}},
    {companionId:'missing-height',screen:{width:480}},
    {companionId:'null',screen:null},
    {companionId:'',screen:{width:480,height:320}},
    {screen:{width:480,height:320}},
    {companionId:'duplicate',screen:{width:480,height:320}},
    {companionId:'duplicate',screen:{width:800,height:480}},
  ]}};
  assert.deepEqual(knownPanelScreenChoices(state),[{id:'duplicate',name:'duplicate',width:480,height:320}]);
});

test('custom panel size validation matches document integer bounds', () => {
  for(const dimensions of [[320,320],[8192,8192],[390,844],[480,320]])assert.equal(panelScreenSizeReason(...dimensions),'');
  for(const dimensions of [[0,320],[319,320],[320,8193],[480.5,320],[NaN,320],[320,Infinity],['480',320]])
    assert.match(panelScreenSizeReason(...dimensions),/whole numbers between 320 and 8192/);
});
