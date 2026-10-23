import type { Expression } from '../../widget-runtime/src/behavior.ts';
import type { BehaviorDefinition } from '../../widget-runtime/src/behavior.ts';
import { encodeWidgetBase64 } from '../../widget-runtime/src/artifact.ts';
import type { WidgetSource } from './types.ts';
const ref=(name:string):Expression=>({ref:name}),literal=(value:string|number|boolean):Expression=>({literal:value});
const choose=(condition:Expression,yes:string|number|boolean,no:string|number|boolean):Expression=>({op:'if',condition,then:literal(yes),else:literal(no)});
/** The sole shipped seed: a capsule with state-owned intent and independent confirmation. */
export function createToggleSource():WidgetSource {
  return {
  "id": "toggle",
  "title": "Toggle",
  "size": {
    "width": 240,
    "height": 144
  },
  "parameters": [
    {
      "id": "label",
      "label": "Title",
      "type": "text",
      "default": "TOGGLE"
    },
    {
      "id": "accent",
      "label": "Thumb",
      "type": "color",
      "default": "#bde7ec"
    },
    {
      "id": "background",
      "label": "Track",
      "type": "color",
      "default": "#252d30"
    },
    {
      "id": "foreground",
      "label": "Text",
      "type": "color",
      "default": "#d2edf0"
    }
  ],
  "behavior": {
    "version": 1,
    "signals": [
      {
        "id": "requested",
        "source": "local",
        "type": "boolean",
        "initial": false
      },
      {
        "id": "confirmed",
        "source": "feedback",
        "type": "boolean",
        "initial": false,
        "freshnessMs": 1000,
        "qualitySignal": "quality",
        "requireGameConfirmed": true
      },
      {
        "id": "quality",
        "source": "input",
        "type": "enum",
        "initial": "unknown",
        "values": [
          "fresh",
          "unknown",
          "stale",
          "disconnected"
        ]
      },
      {
        "id": "pressed",
        "source": "local",
        "type": "boolean",
        "initial": false
      },
      {
        "id": "available",
        "source": "input",
        "type": "boolean",
        "initial": true
      }
    ],
    "events": [
      {
        "id": "activate"
      },
      {
        "id": "press"
      },
      {
        "id": "release"
      },
      {
        "id": "cancel"
      },
      {
        "id": "feedback",
        "updates": [
          "confirmed"
        ]
      },
      {
        "id": "availability",
        "updates": [
          "available"
        ]
      }
    ],
    "outputs": [
      {
        "id": "toggle",
        "type": "boolean",
        "neutral": false
      }
    ],
    "regions": [
      {
        "id": "interaction",
        "initial": "ready",
        "states": [
          "ready"
        ],
        "transitions": [
          {
            "id": "press",
            "from": "ready",
            "event": "press",
            "guard": {
              "ref": "available"
            },
            "actions": [
              {
                "type": "set",
                "signal": "pressed",
                "value": {
                  "literal": true
                }
              }
            ]
          },
          {
            "id": "release",
            "from": "ready",
            "event": "release",
            "actions": [
              {
                "type": "set",
                "signal": "pressed",
                "value": {
                  "literal": false
                }
              }
            ]
          },
          {
            "id": "cancel",
            "from": "ready",
            "event": "cancel",
            "actions": [
              {
                "type": "set",
                "signal": "pressed",
                "value": {
                  "literal": false
                }
              }
            ]
          }
        ]
      },
      {
        "id": "requested-state",
        "initial": "off",
        "states": [
          "off",
          "on"
        ],
        "transitions": [
          {
            "id": "turn-on",
            "from": "off",
            "to": "on",
            "event": "activate",
            "guard": {
              "ref": "available"
            },
            "actions": [
              {
                "type": "cue",
                "cue": "click"
              }
            ]
          },
          {
            "id": "turn-off",
            "from": "on",
            "to": "off",
            "event": "activate",
            "guard": {
              "ref": "available"
            },
            "actions": [
              {
                "type": "cue",
                "cue": "click"
              }
            ]
          }
        ]
      }
    ]
  },
  "visualDomains": [],
  "inputs": [
    {
      "id": "toggle",
      "label": "Toggle",
      "type": "boolean",
      "signal": "requested"
    }
  ],
  "outputs": [
    {
      "id": "confirmation",
      "label": "Game confirmation",
      "type": "boolean",
      "signal": "confirmed"
    }
  ],
  "hitRegions": [
    {
      "id": "surface",
      "x": 0,
      "y": 0,
      "width": 240,
      "height": 144,
      "events": {
        "press": "press",
        "release": "release",
        "activate": "activate",
        "cancel": "cancel"
      }
    }
  ],
  "scene": [
    {
      "id": "title",
      "kind": "text",
      "x": 120,
      "y": 27,
      "text": {
        "ref": "label"
      },
      "font": "sans",
      "fontSize": 28,
      "align": "center",
      "fill": {
        "ref": "foreground"
      }
    },
    {
      "id": "body",
      "kind": "group",
      "opacity": {
        "op": "if",
        "condition": {
          "ref": "available"
        },
        "then": {
          "literal": 1
        },
        "else": {
          "literal": 0.4
        }
      },
      "children": [
        {
          "id": "track-halo",
          "kind": "rectangle",
          "x": 23,
          "y": 47,
          "width": 194,
          "height": 66,
          "radius": 33,
          "fill": "none",
          "stroke": {
            "ref": "accent"
          },
          "strokeWidth": 6,
          "opacity": 0.08
        },
        {
          "id": "track",
          "kind": "rectangle",
          "x": 24,
          "y": 48,
          "width": 192,
          "height": 64,
          "radius": 32,
          "fill": {
            "ref": "background"
          },
          "stroke": {
            "ref": "accent"
          },
          "strokeWidth": {
            "op": "if",
            "condition": {
              "ref": "pressed"
            },
            "then": {
              "literal": 3
            },
            "else": {
              "literal": 2
            }
          }
        },
        {
          "id": "thumb-halo",
          "kind": "ellipse",
          "x": 25,
          "y": 49,
          "width": 62,
          "height": 62,
          "fill": {
            "ref": "accent"
          },
          "opacity": 0.12
        },
        {
          "id": "thumb",
          "kind": "ellipse",
          "x": 31,
          "y": 55,
          "width": 50,
          "height": 50,
          "fill": {
            "ref": "accent"
          }
        },
        {
          "id": "thumb-highlight",
          "kind": "ellipse",
          "x": 38,
          "y": 62,
          "width": 36,
          "height": 36,
          "fill": "#ffffff",
          "opacity": 0.12
        }
      ]
    },
    {
      "id": "confirmation",
      "kind": "text",
      "x": 120,
      "y": 135,
      "text": {
        "op": "if",
        "condition": {
          "op": "eq",
          "left": {
            "ref": "quality"
          },
          "right": {
            "literal": "fresh"
          }
        },
        "then": {
          "op": "if",
          "condition": {
            "ref": "confirmed"
          },
          "then": {
            "literal": "Game: ON"
          },
          "else": {
            "literal": "Game: OFF"
          }
        },
        "else": {
          "op": "if",
          "condition": {
            "op": "eq",
            "left": {
              "ref": "quality"
            },
            "right": {
              "literal": "stale"
            }
          },
          "then": {
            "literal": "Game: stale"
          },
          "else": {
            "op": "if",
            "condition": {
              "op": "eq",
              "left": {
                "ref": "quality"
              },
              "right": {
                "literal": "disconnected"
              }
            },
            "then": {
              "literal": "Game: disconnected"
            },
            "else": {
              "literal": "Game: unknown"
            }
          }
        }
      },
      "font": "mono",
      "fontSize": 11,
      "align": "center",
      "fill": {
        "ref": "foreground"
      }
    }
  ],
  "motions": [],
  "sounds": [],
  "assets": [],
  "design": {
    "nodes": [],
    "owners": [
      {
        "region": "requested-state",
        "signal": "requested",
        "values": {
          "off": false,
          "on": true
        },
        "port": "toggle"
      }
    ],
    "appearances": [
      {
        "id": "off",
        "regions": {
          "requested-state": "off"
        },
        "layers": {}
      },
      {
        "id": "on",
        "regions": {
          "requested-state": "on"
        },
        "layers": {
          "thumb-halo": {
            "x": 151
          },
          "thumb": {
            "x": 157
          },
          "thumb-highlight": {
            "x": 164
          }
        }
      }
    ],
    "effects": [
      {
        "region": "requested-state",
        "transition": "turn-on",
        "durationMs": 160,
        "fps": 30,
        "properties": [
          {
            "node": "thumb",
            "property": "x"
          },
          {
            "node": "thumb-halo",
            "property": "x"
          },
          {
            "node": "thumb-highlight",
            "property": "x"
          }
        ],
        "easing": "ease-in-out",
        "interruption": "reverse"
      },
      {
        "region": "requested-state",
        "transition": "turn-off",
        "durationMs": 160,
        "fps": 30,
        "properties": [
          {
            "node": "thumb",
            "property": "x"
          },
          {
            "node": "thumb-halo",
            "property": "x"
          },
          {
            "node": "thumb-highlight",
            "property": "x"
          }
        ],
        "easing": "ease-in-out",
        "interruption": "reverse"
      }
    ]
  }
};
}
/** New generalization fixtures are deliberately outside the shipped seed catalog. */
function gaugeBehavior():BehaviorDefinition{
  const fresh:Expression={op:'eq',left:ref('quality'),right:literal('fresh')};
  return {version:1,signals:[{id:'rpm',source:'feedback',type:'number',initial:0,min:0,max:8000,freshnessMs:500,qualitySignal:'quality',requireGameConfirmed:true},{id:'quality',source:'input',type:'enum',initial:'unknown',values:['fresh','unknown','stale','disconnected']}],events:[{id:'feedback',updates:['rpm']},{id:'feedback-expired'}],timers:[{id:'freshness',event:'feedback-expired',delayMs:500}],regions:[
    {id:'alarm',initial:'normal',states:['normal','redline'],transitions:[
      {id:'redline-enter',from:'normal',event:'feedback',guard:{op:'and',values:[fresh,{op:'gte',left:ref('rpm'),right:literal(7000)}]},to:'redline',actions:[{type:'cue',cue:'redline'}]},
      {id:'redline-exit',from:'redline',event:'feedback',guard:{op:'or',values:[{op:'not',value:fresh},{op:'lte',left:ref('rpm'),right:literal(6800)}]},to:'normal'},
      {id:'expired',from:'*',event:'feedback-expired',to:'normal'},
    ]},
    {id:'sampling',initial:'ready',states:['ready'],transitions:[{id:'received',from:'ready',event:'feedback',actions:[{type:'start-timer',timer:'freshness'}]}]},
  ]};
}
function gaugeStatus():Expression{return {op:'if',condition:{op:'eq',left:ref('quality'),right:literal('fresh')},then:literal('RPM'),else:{op:'if',condition:{op:'eq',left:ref('quality'),right:literal('stale')},then:literal('RPM stale'),else:{op:'if',condition:{op:'eq',left:ref('quality'),right:literal('disconnected')},then:literal('RPM disconnected'),else:literal('RPM unknown')}}};}
function gaugeColor(normal:Expression|string):Expression{return {op:'if',condition:{op:'eq',left:{state:'alarm'},right:literal('redline')},then:literal('#f07373'),else:typeof normal==='string'?literal(normal):normal};}
export function createLinearGaugeFixture():WidgetSource{
  return {id:'fixture-linear-rpm',title:'Linear RPM fixture',size:{width:256,height:64},parameters:[{id:'accent',label:'Fill',type:'color',default:'#71e5c3'}],behavior:gaugeBehavior(),visualDomains:[{signal:'rpm',min:0,max:8000,samples:129,maxErrorPixels:1,travelPixels:240}],inputs:[],outputs:[{id:'rpm',label:'RPM',type:'number',signal:'rpm',min:0,max:8000}],hitRegions:[],scene:[{id:'track',kind:'rectangle',x:8,y:16,width:240,height:32,radius:4,fill:'#20313c'},{id:'fill',kind:'rectangle',x:8,y:16,width:{op:'mul',left:{op:'div',left:ref('rpm'),right:literal(8000)},right:literal(240)},height:32,fill:gaugeColor(ref('accent'))},{id:'quality-label',kind:'text',x:8,y:12,text:gaugeStatus(),font:'mono',fontSize:10,fill:'#d2edf0'}],motions:[],sounds:[]};
}
export function createRadialGaugeFixture():WidgetSource{
  return {id:'fixture-radial-rpm',title:'Radial RPM fixture',size:{width:192,height:192},parameters:[],behavior:gaugeBehavior(),visualDomains:[{signal:'rpm',min:0,max:8000,samples:97,maxErrorPixels:2,travelPixels:72*Math.PI*1.5}],inputs:[],outputs:[{id:'rpm',label:'RPM',type:'number',signal:'rpm',min:0,max:8000}],hitRegions:[],scene:[{id:'dial',kind:'ellipse',x:8,y:8,width:176,height:176,fill:'#182630',stroke:'#557075',strokeWidth:2},{id:'needle',kind:'line',x:96,y:96,x2:96,y2:24,stroke:gaugeColor('#71e5c3'),strokeWidth:3,transform:{originX:96,originY:96,rotate:{op:'add',left:literal(-135),right:{op:'mul',left:{op:'div',left:ref('rpm'),right:literal(8000)},right:literal(270)}}}},{id:'hub',kind:'ellipse',x:91,y:91,width:10,height:10,fill:'#d2edf0'},{id:'quality-label',kind:'text',x:96,y:181,text:gaugeStatus(),font:'mono',fontSize:10,align:'center',fill:'#d2edf0'}],motions:[],sounds:[]};
}
export function createThreeStateFixture():WidgetSource{
  const next:Expression={op:'if',condition:{op:'eq',left:ref('position'),right:literal('low')},then:literal('middle'),else:{op:'if',condition:{op:'eq',left:ref('position'),right:literal('middle')},then:literal('high'),else:literal('low')}};
  return {id:'fixture-three-position',title:'Three-position fixture',size:{width:192,height:64},parameters:[],behavior:{version:1,signals:[{id:'position',source:'local',type:'enum',initial:'low',values:['low','middle','high']}],events:[{id:'activate'}],outputs:[{id:'select',type:'enum',neutral:'low',values:['low','middle','high']}],regions:[{id:'selection',initial:'ready',states:['ready'],transitions:[{id:'next',from:'ready',event:'activate',actions:[{type:'set',signal:'position',value:next},{type:'emit',port:'select',value:next}]}]}]},visualDomains:[],inputs:[{id:'select',label:'Position',type:'enum',signal:'position',values:['low','middle','high']}],outputs:[],hitRegions:[{id:'surface',x:0,y:0,width:192,height:64,events:{activate:'activate'}}],scene:[{id:'track',kind:'rectangle',x:2,y:2,width:188,height:60,radius:20,fill:'#20313c'},{id:'marker',kind:'ellipse',x:{op:'if',condition:{op:'eq',left:ref('position'),right:literal('low')},then:literal(10),else:{op:'if',condition:{op:'eq',left:ref('position'),right:literal('middle')},then:literal(72),else:literal(134)}},y:12,width:40,height:40,fill:'#71e5c3'}],motions:[],sounds:[]};
}
/** Small deterministic PCM fixture; optional sound assets remain ordinary author data. */
export function createPcmSoundFixture():{id:string;mime:'audio/wav';base64:string}{
  const samples=800,bytes=new Uint8Array(44+samples*2),view=new DataView(bytes.buffer);const tag=(offset:number,text:string)=>{for(let i=0;i<text.length;i++)bytes[offset+i]=text.charCodeAt(i);};tag(0,'RIFF');view.setUint32(4,bytes.length-8,true);tag(8,'WAVE');tag(12,'fmt ');view.setUint32(16,16,true);view.setUint16(20,1,true);view.setUint16(22,1,true);view.setUint32(24,16000,true);view.setUint32(28,32000,true);view.setUint16(32,2,true);view.setUint16(34,16,true);tag(36,'data');view.setUint32(40,samples*2,true);for(let index=0;index<samples;index++)view.setInt16(44+index*2,Math.round(Math.sin(index*Math.PI/8)*2000*(1-index/samples)),true);return {id:'click-wave',mime:'audio/wav',base64:encodeWidgetBase64(bytes)};
}
