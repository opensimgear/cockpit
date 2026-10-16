import {test} from 'node:test';
import assert from 'node:assert/strict';
import {feedbackBindingError,feedbackSourceKey,feedbackSourceFromKey} from './companion-feedback.ts';
const instance={instanceId:'a',publicationHash:'sha256:'+'a'.repeat(64),outputs:[{outputId:'flag',channelId:'flag-channel',kind:'indicator',unit:'state'},{outputId:'rpm',channelId:'rpm-channel',kind:'gauge',unit:'rpm',range:{min:0,max:8000}},{outputId:'label',channelId:'label-channel',kind:'text',unit:'text'}],controls:[{id:'button',label:'Button',kind:'button'},{id:'axis',label:'Axis',kind:'axis',axisMode:'unipolar'},{id:'hat',label:'Hat',kind:'hat'}],bindings:[],configured:false,pinned:false};
const sources=[{bindingId:'engine.rpm',label:'RPM',kind:'integer',unit:'rpm',minimum:0,maximum:8000,confirmed:true},{bindingId:'label',label:'Label',kind:'string',unit:'text',confirmed:false},{bindingId:'flag',label:'Flag',kind:'boolean',unit:'state',confirmed:true}];
const route=(outputId,source,extra={})=>({outputId,channelId:instance.outputs.find(o=>o.outputId===outputId).channelId,source,...extra});
test('feedback bindings preserve exact per-instance output identity and typed unit/range semantics',()=>{
 const bindings=[route('flag',{kind:'requested',controlId:'button'}),route('rpm',{kind:'integration',bindingId:'engine.rpm'}),route('label',{kind:'integration',bindingId:'label'})];assert.equal(feedbackBindingError(instance,bindings,sources),'');
 assert.match(feedbackBindingError(instance,[...bindings,bindings[0]],sources),/at most once/);assert.match(feedbackBindingError(instance,[{...bindings[0],outputId:'wrong'}],sources),/identity/);
 assert.match(feedbackBindingError(instance,[route('rpm',{kind:'requested',controlId:'axis'})],sources),/Units differ/);assert.equal(feedbackBindingError(instance,[route('rpm',{kind:'requested',controlId:'axis'},{scale:8000})],sources),'');
 assert.match(feedbackBindingError(instance,[route('rpm',{kind:'requested',controlId:'axis'},{scale:9000})],sources),/converted source range/);
 assert.match(feedbackBindingError(instance,[route('flag',{kind:'integration',bindingId:'label'})],sources),/type/);assert.match(feedbackBindingError(instance,[route('label',{kind:'integration',bindingId:'engine.rpm'})],sources),/type/);
 assert.match(feedbackBindingError(instance,[route('flag',{kind:'requested',controlId:'button'},{scale:2})],sources),/numeric/);
 assert.equal(feedbackBindingError(instance,[route('rpm',{kind:'host-accepted',controlId:'hat'},{scale:1000,offset:1000})],sources),'');
});
test('invalid route sources and bounds fail before the advisory save request',()=>{
 assert.match(feedbackBindingError({...instance,publicationHash:'bad'},[],sources),/verified/);assert.match(feedbackBindingError(instance,Array(65).fill({}),sources),/64/);
 for(const source of [{kind:'integration',bindingId:'missing'},{kind:'requested',controlId:'missing'},{kind:'unknown',controlId:'button'},{kind:'requested',controlId:'bad id'}])assert.ok(feedbackBindingError(instance,[route('flag',source)],sources));
 for(const extra of [{scale:NaN},{scale:1e7},{offset:Infinity},{offset:1e10}])assert.match(feedbackBindingError(instance,[route('rpm',{kind:'integration',bindingId:'engine.rpm'},extra)],sources),/finite/);
 assert.match(feedbackBindingError(instance,[route('rpm',{kind:'integration',bindingId:'flag'})],sources),/type/);
 assert.equal(feedbackBindingError(instance,[route('rpm',{kind:'integration',bindingId:'engine.rpm'})],sources.map(source=>({...source,minimum:null,maximum:null}))),'');
});
test('source selection roundtrips stage and full stable identity without name aliasing',()=>{
 for(const source of [{kind:'integration',bindingId:'engine.rpm'},{kind:'requested',controlId:'axis'},{kind:'host-accepted',controlId:'axis'},{kind:'host-submitted',controlId:'axis'}])assert.deepEqual(feedbackSourceFromKey(feedbackSourceKey(source)),source);
 for(const value of ['bad','requested:','other:axis','integration:bad id','requested:bad id'])assert.equal(feedbackSourceFromKey(value),null);
});
