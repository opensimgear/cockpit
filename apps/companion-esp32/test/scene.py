"""Portable scene admission/composition regression and independent ABI vectors."""
from pathlib import Path
import struct, json, subprocess, os, gzip
ROOT=Path(__file__).resolve().parents[3]
APP=ROOT/'apps/companion-esp32'
OUT=ROOT/'.research/esp32-render-scene-native'
OUT.mkdir(parents=True,exist_ok=True)
ABI=json.loads((ROOT/'packages/companion-renderer/test/fixtures/abi.json').read_text())['recordBytes']
def record(size,fields):
    data=bytearray(size)
    for at,fmt,value in fields: struct.pack_into('<'+fmt,data,at,value)
    return data
def pack(kind,sections):
    sections=[(k,bytes(v)) for k,v in sorted(sections.items()) if v]
    cursor=32+len(sections)*16
    entries=[]
    for key,data in sections:
        cursor=(cursor+3)&~3
        stride=ABI[kind][str(key)]
        assert len(data)%stride==0
        entries.append((key,stride,cursor,len(data)//stride,len(data),data))
        cursor+=len(data)
    data=bytearray(cursor)
    struct.pack_into('<4sHHIIHH',data,0,{'semantics':b'OSGC','scene':b'OSGS','images':b'OSGI','fonts':b'OSGF'}[kind],1,32,cursor,32,len(entries),0)
    for i,(key,stride,offset,count,length,raw) in enumerate(entries):
        struct.pack_into('<HHIII',data,32+i*16,key,stride,offset,count,length)
        data[offset:offset+length]=raw
    return data
strings=bytearray(b'firetargetpowerchannelmainTEXT')
sem={1:record(32,[(0,'I',0),(4,'H',4),(6,'B',1),(8,'I',4),(12,'H',6),(20,'i',65536),(30,'H',65535)]),2:record(32,[(0,'I',10),(4,'H',5),(6,'B',2),(8,'I',15),(12,'H',7),(20,'i',65536)]),3:record(24,[(0,'I',0),(4,'H',4),(6,'B',1),(10,'H',65535),(12,'H',65535),(14,'H',65535)]),4:strings}
def node(op,x,y,w,h,extra=()):
    return record(64,[(2,'B',op),(3,'B',1),(4,'h',x),(6,'h',y),(8,'H',w),(10,'H',h),(12,'h',0),(14,'h',24),(16,'H',480),(18,'H',288),(20,'h',x),(22,'h',y),(24,'H',w),(26,'H',h),(28,'H',65535),(30,'H',65535),(32,'H',65535),(36,'H',65535),*extra])
nodes=[node(1,0,24,44,44),node(2,64,24,3,2,[(28,'H',0)]),node(3,68,24,3,2,[(30,'H',1)]),node(4,72,24,3,2,[(28,'H',0),(30,'H',1)]),node(5,0,80,64,16,[(32,'H',0),(42,'H',1),(44,'I',4),(48,'H',4),(50,'H',120)]),node(6,96,80,32,16,[(34,'H',1),(38,'H',0),(40,'H',2),(36,'H',2047)]),node(7,128,80,32,16,[(34,'H',2),(38,'H',2),(40,'H',3),(36,'H',63488)]),node(1,160,80,44,44)]
scene={1:record(16,[(0,'I',0),(4,'H',4),(8,'H',len(nodes)),(10,'H',4226)]),2:b''.join(nodes),3:record(24,[(2,'B',2),(12,'i',65536)])+record(24,[(2,'B',1),(3,'B',1),(12,'i',65536)])+record(24,[(2,'B',1),(3,'B',2),(12,'i',65536)])+record(24,[(2,'B',1),(3,'B',3),(12,'i',65536)])+record(24,[(0,'H',65535),(2,'B',1),(3,'B',4),(12,'i',65536)]),4:record(16,[(2,'B',4),(6,'H',4),(12,'i',65535)])+record(16,[(0,'H',4),(2,'B',6),(4,'H',4),(6,'H',1),(12,'i',65536)]),5:record(16,[(0,'B',2),(2,'H',1)])+record(16,[(0,'B',1),(4,'i',63488)])+record(16,[(0,'B',1),(4,'i',2016)])+record(16,[(0,'B',14)])+record(16,[(0,'B',2)]),6:record(24,[(0,'H',7),(2,'B',1),(6,'H',3),(8,'H',100),(10,'H',1)]),7:record(8,[(4,'i',65536)])+record(8,[(0,'H',50)])+record(8,[(0,'H',100),(4,'i',65536)]),8:record(24,[(4,'h',0),(6,'h',24),(8,'H',44),(10,'H',44)]),9:b''.join(record(4,[(0,'h',x),(2,'h',y)]) for x,y in [(98,82),(120,90),(130,82),(142,82),(142,92)]),10:b'mainTEXT'}
tiles={}
for index,data in enumerate(nodes):
    x,y,w,h=struct.unpack_from('<hhHH',data,20)
    for ty in range((y-24)//8,(y+h-1-24)//8+1):
        for tx in range(x//64,(x+w-1)//64+1):tiles.setdefault((ty,tx),[]).append(index)
indices=[];tiledata=[]
for (ty,tx),members in sorted(tiles.items()):
    tiledata.append(record(12,[(2,'H',tx),(4,'H',ty),(6,'H',len(indices)),(8,'H',len(members))]));indices+=members
scene[11]=b''.join(tiledata);scene[12]=b''.join(struct.pack('<H',i) for i in indices)
images={1:record(24,[(0,'H',3),(2,'H',2),(4,'I',6),(12,'I',12),(16,'B',1)])+record(24,[(0,'H',3),(2,'H',2),(4,'I',2),(8,'I',12),(12,'I',4),(16,'B',2)]),2:struct.pack('<6H',63488,2016,31,65535,0,2047)+bytes([0xf0,0x80,0x0f,0xf0])}
fonts={1:record(16,[(2,'H',1),(6,'H',8)]),2:record(24,[(0,'I',63),(4,'H',3),(6,'H',2),(12,'h',4),(20,'I',4)]),3:bytes([0xff,0xf0,0xff,0xf0])}
def write_case(name,semantics=sem,visual=scene):
    folder=OUT/name;folder.mkdir(exist_ok=True)
    for kind,data in [('semantics',semantics),('scene',visual),('images',images),('fonts',fonts)]: (folder/(kind+'.bin')).write_bytes(pack(kind,data))
    return folder
write_case('base')
lines=['static void interval_vectors(void) {']
fixtures=json.loads((ROOT/'packages/companion-renderer/test/fixtures/expressions.json').read_text())
for index,vector in enumerate(fixtures['vectors']):
    if vector['expected'] is None:continue
    semantic={**sem};visual={**scene};text=bytearray(strings);outputs=[];variables=[]
    for i,value in enumerate(vector['variables']):
        name=('v'+str(i)).encode();offset=len(text);text+=name
        outputs.append(record(32,[(0,'I',offset),(4,'H',len(name)),(6,'B',2),(8,'I',offset),(12,'H',len(name)),(16,'i',value),(20,'i',value)]))
        variables.append(record(24,[(0,'H',i),(2,'B',2),(4,'i',value),(8,'i',value),(12,'i',value)]))
    semantic[2]=b''.join(outputs);semantic[4]=text;visual[3]=b''.join(variables)
    narrow=not vector['variables'] and len(vector['instructions'])==2 and vector['instructions'][0]['op']==1 and vector['instructions'][1]['op']==16
    lower=vector['expected'] if narrow else -2147483648;upper=vector['expected'] if narrow else 2147483647
    visual[4]=record(16,[(0,'H',4),(2,'B',6),(6,'H',len(vector['instructions'])),(8,'i',lower),(12,'i',upper)])
    visual[5]=b''.join(record(16,[(0,'B',i['op']),(2,'H',i.get('operand',0)),(4,'i',i.get('value',0))]) for i in vector['instructions'])
    visual[13]=b''.join(struct.pack('<ii',x,y) for x,y in vector['lookup'])
    folder=write_case('expression-'+str(index),semantic,visual)
    lines.append('load_case('+json.dumps(str(folder))+', &OSG_SCENE_WAVESHARE); assert(osg_scene_admit(&engine,sources,&OSG_SCENE_WAVESHARE));')
lines.append('}')
poc=ROOT/'packages/companion-renderer/test/poc'
cases=json.loads((poc/'cases.json').read_text())
lines.append('static void poc_vectors(void) {')
for case in cases['cases']:
    profile='&OSG_SCENE_SUNTON' if case['profile']=='sunton' else '&OSG_SCENE_WAVESHARE'
    folder=poc/case['package']
    manifest=json.loads((folder/'manifest.json').read_text())
    lines.append('load_case('+json.dumps(str(folder))+', '+profile+'); assert(osg_scene_admit(&engine,sources,'+profile+'));')
    lines.append('{ osg_scene_usage usage; assert(osg_scene_get_usage(&engine,&usage));')
    for key,field in {'pages':'pages','nodes':'nodes','variables':'variables','bindings':'bindings','hitRegions':'hit_regions','glyphs':'glyphs','timelines':'timelines'}.items():
        lines.append('assert(usage.'+field+'=='+str(manifest['counts'][key])+');')
    for key,field in {'maxPageDrawOps':'max_page_draw_ops','maxUpdateInstructions':'max_update_instructions','maxDirtyPixels':'max_dirty_pixels','maxLayersPerTile':'max_layers_per_tile','maxBlendPixelLayers':'max_blend_pixel_layers'}.items():
        lines.append('assert(usage.'+field+'=='+str(manifest['resources'][key])+');')
    lines.append('assert(usage.minimum_snapshot_bytes<='+str(manifest['resources']['snapshotBytes'])+'&&usage.runtime_bytes<='+str(manifest['resources']['internalRamBytes'])+');}')
    for frame in case['frames']:
        state=frame['state']
        framepath=folder/frame['rgb565']
        if framepath.suffix=='.gz':
            raw=OUT/('poc-'+case['package'].replace('/','-')+'-'+frame['name']+'.rgb565')
            raw.write_bytes(gzip.decompress(framepath.read_bytes()));framepath=raw
        lines.append('{')
        feedback=state['feedback']
        rows=['{(osg_expression_type)'+str(v['type'])+','+str(v['available']).lower()+','+str(v['fresh']).lower()+','+str(float(v.get('value',0)))+'}' for v in feedback]
        lines.append('osg_scene_feedback feedback[]={'+','.join(rows)+'};')
        lines.append('bool pressed[]={'+','.join(str(v).lower() for v in state['pressed'])+'};')
        lines.append('osg_scene_state state={feedback,'+str(len(feedback))+',pressed,'+str(len(state['pressed']))+','+str(state['connected']).lower()+','+str(state['page'])+','+str(state['timeMs'])+'};')
        lines.append('assert(osg_scene_update(&engine,&state)); if(engine.pending)render(); assert_frame('+json.dumps(str(framepath))+');')
        for hit in frame['hitTests']:
            lines.append('assert(osg_scene_hit_test(&engine,'+str(hit['x'])+','+str(hit['y'])+')=='+str(-1 if hit['expected'] is None else hit['expected'])+');')
        lines.append('}')
lines.append('}')
(OUT/'scene-vectors.h').write_text('\n'.join(lines)+'\n')
for path in OUT.glob('scene-test-*.gcda'):path.unlink()
subprocess.run([os.environ.get('CC','gcc'),'-std=c11','-Wall','-Wextra','-Werror','-pedantic','-g','-fsanitize=address,undefined','--coverage','-I'+str(OUT),str(APP/'render/package.c'),str(APP/'render/expressions.c'),str(APP/'render/scene.c'),str(APP/'test/scene_test.c'),'-lm','-o',str(OUT/'scene-test')],cwd=ROOT,check=True)
result=subprocess.run([str(OUT/'scene-test'),str(OUT/'base')],cwd=ROOT,check=False,capture_output=True,text=True)
(OUT/'scene-test.log').write_text(result.stdout+result.stderr);print(result.stdout+result.stderr,end='')
if result.returncode:raise SystemExit(result.returncode)
# Enumerate the actual tested catalog, including optional package-only revisions.
# This evidence is emitted only after every native full-frame comparison passed.
evidence={'formatVersion':1,'cases':[{'package':case['package'],'frames':len(case['frames'])} for case in cases['cases']]}
(OUT/'poc-evidence.json').write_text(json.dumps(evidence,indent=2)+'\n')
print('Native C/TypeScript RGB565 goldens: '+', '.join(case['package']+' ('+str(len(case['frames']))+' frames)' for case in cases['cases']))
subprocess.run(['gcov','-j','-b','-c','scene-test-scene.gcno'],cwd=OUT,check=True)
report=json.loads(gzip.decompress((OUT/'scene-test-scene.gcov.json.gz').read_bytes()))
module=next(f for f in report['files'] if f['file'].endswith('/render/scene.c'))
lines=module['lines'];functions=module['functions'];branches=[b for l in lines for b in l.get('branches',[])]
metrics={'lines':100*sum(l['count']>0 for l in lines)/len(lines),'branches':100*sum(b['count']>0 for b in branches)/len(branches),'functions':100*sum(f['execution_count']>0 for f in functions)/len(functions)}
(OUT/'coverage.json').write_text(json.dumps({'render-scene':metrics},indent=2)+'\n');print(json.dumps(metrics,indent=2))
if any(v<71 for v in metrics.values()):raise SystemExit('Scene coverage below71%')