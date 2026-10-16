import test from 'node:test';
import assert from 'node:assert/strict';
import { selectedUsbIsPaired, newPairingConnected } from './companion-pairing.ts';

test('an existing USB companion cannot clear a different candidate selection',()=>{
 const state={usb:{connected:true,port:'COM5',device:{companionId:'waveshare',setupSaved:true}},host:{clients:[{companionId:'waveshare',connected:true,kind:'esp32'}]}};
 assert.equal(selectedUsbIsPaired(state,'COM6'),false);
 assert.equal(selectedUsbIsPaired(state,'COM5'),true);
 assert.equal(selectedUsbIsPaired(state,''),false);
 assert.equal(selectedUsbIsPaired({...state,host:{clients:[]}},'COM5'),false);
});

test('only the newly provisioned USB identity completes its wizard',()=>{
 const state={usb:{connected:true,port:'COM5',device:{companionId:'waveshare',setupSaved:true}},host:{clients:[{companionId:'waveshare',connected:true,kind:'esp32'},{companionId:'browser',connected:true,kind:'browser'}]}};
 const baseline=new Set(['waveshare']);
 assert.equal(newPairingConnected('esp32',baseline,state),false);
 assert.equal(newPairingConnected('',baseline,state),false);
 const sunton={...state,usb:{connected:true,port:'COM6',device:{companionId:'sunton',setupSaved:true}},host:{clients:[...state.host.clients,{companionId:'sunton',connected:true,kind:'esp32'}]}};
 assert.equal(newPairingConnected('esp32',baseline,sunton),true);
 assert.equal(newPairingConnected('esp32',baseline,{...sunton,usb:{...sunton.usb,device:{companionId:'sunton',setupSaved:false}}}),false);
 assert.equal(newPairingConnected('browser',baseline,sunton),true);
 assert.equal(newPairingConnected('browser',new Set(['waveshare','browser']),sunton),false);
});
