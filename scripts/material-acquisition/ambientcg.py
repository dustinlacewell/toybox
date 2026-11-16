"""Fetch small CC0 AmbientCG PBR sets into a Toybox library, preserving its manifest."""
import collections, concurrent.futures, hashlib, io, json, os, pathlib, re, shutil, time, zipfile
import requests
from PIL import Image

ROOT = pathlib.Path(r'D:\code\games\assets')
PACK = ROOT / 'library' / 'ambientcg'
MANIFEST = ROOT / '_library_config' / 'catalog.json'
RESERVE = 5 * 1024**3
UA = 'ToyboxPersonalMaterialImporter/1.0 (CC0 personal library)'
session = requests.Session()
session.headers['User-Agent'] = UA

def get(url):
    for attempt in range(3):
        try:
            r = session.get(url, timeout=90)
            if r.status_code == 429:
                time.sleep(15 * (attempt + 1)); continue
            r.raise_for_status()
            return r
        except requests.RequestException:
            if attempt == 2: raise
            time.sleep(2 * (attempt + 1))
    raise RuntimeError('rate limited')

def catalog():
    url = 'https://ambientcg.com/api/v3/assets?type=material&include=downloads&limit=100'
    items = []
    while url:
        page = get(url).json(); items.extend(page['assets']); url = page['nextPageHttp']
    print('discovered',len(items),flush=True)
    groups = collections.defaultdict(list)
    for a in items:
        dl = next((d for d in a['downloads'] if d['attributes'] == '1K-JPG' and d['extension'] == 'zip'), None)
        if dl:
            group = re.match(r'[A-Za-z]+', a['id']).group().lower()
            groups[group].append((dl['size'], a['id'], dl['url']))
    print('1k jpg sets',sum(map(len,groups.values())),'archive bytes',sum(x[0] for g in groups.values() for x in g),flush=True)
    for group in groups.values(): group.sort()
    # Round robin by category, prioritizing the smallest in each group.
    for i in range(max(map(len,groups.values()))):
        for name in sorted(groups):
            if i < len(groups[name]): yield name, *groups[name][i]

def fetch(item):
    category, size, name, url = item
    data = get(url).content
    if len(data) != size: raise ValueError('archive size mismatch '+name)
    maps = {}
    role_names = {'color':'albedo','normalgl':'normal','roughness':'roughness','metalness':'metallic','metallic':'metallic','ambientocclusion':'ao','displacement':'height','emission':'emissive','opacity':'opacity'}
    with zipfile.ZipFile(io.BytesIO(data)) as z:
        for info in z.infolist():
            if info.is_dir(): continue
            fn = pathlib.PurePosixPath(info.filename).name
            if not fn.lower().endswith(('.jpg','.png')): continue
            suffix = pathlib.Path(fn).stem.rsplit('_', 1)[-1].lower()
            role = role_names.get(suffix)
            if not role or role in maps: continue
            payload = z.read(info)
            with Image.open(io.BytesIO(payload)) as img:
                img.verify()
            with Image.open(io.BytesIO(payload)) as img:
                if min(img.size) < 1 or max(img.size) > 16384: raise ValueError('unexpected image dimensions '+fn)
                img.load()
            maps[role] = (fn,payload)
    if 'albedo' not in maps: raise ValueError('missing color map '+name+' '+str(list(maps)))
    return category,name,url,maps

def atomic_json(path, obj):
    data = (json.dumps(obj, indent=2, ensure_ascii=False)+'\n').encode('utf-8')
    tmp = path.with_name(path.name+'.ambientcg.tmp')
    tmp.write_bytes(data); os.replace(tmp,path)

def save(result):
    category,name,url,maps = result
    dest = PACK/'materials'/name
    dest.mkdir(parents=True,exist_ok=True)
    for fn,payload in maps.values():
        p = dest/fn
        if p.exists():
            if hashlib.sha256(p.read_bytes()).digest()!=hashlib.sha256(payload).digest(): raise ValueError('existing file differs: '+str(p))
        else:
            with p.open('xb') as f: f.write(payload)
    provenance = {'provider':'ambientCG','asset':name,'source':'https://ambientcg.com/a/'+name,'download':url,'license':'CC0-1.0','license_url':'https://docs.ambientcg.com/license/'}
    prov = dest/'provenance.json'
    if not prov.exists(): atomic_json(prov,provenance)
    elif json.loads(prov.read_text(encoding='utf-8')) != provenance: raise ValueError('existing provenance differs '+name)
    return {'id':'uid://ambientcg_'+name.lower(),'kind':'material','pack':'ambientcg','category':category,'name':name,'maps':[{'role':role,'file':x[0]} for role,x in maps.items()]}

def commit(entries):
    if not entries: return
    original = MANIFEST.read_bytes()
    doc = json.loads(original.decode('utf-8-sig'))
    known = {a['id'] for a in doc['assets']}
    doc['assets'].extend(x for x in entries if x['id'] not in known)
    if MANIFEST.read_bytes()!=original: raise RuntimeError('manifest changed concurrently; refusing overwrite')
    atomic_json(MANIFEST,doc)

def main():
    PACK.mkdir(parents=True,exist_ok=True)
    packmeta = PACK/'pack.json'
    if not packmeta.exists(): atomic_json(packmeta,{'slug':'ambientcg','name':'ambientCG (CC0)','color':'668b72ff'})
    elif json.loads(packmeta.read_text(encoding='utf-8')).get('slug') != 'ambientcg': raise RuntimeError('pack conflict')
    backup = MANIFEST.with_name('catalog.before-ambientcg-'+time.strftime('%Y%m%d-%H%M%S')+'.json')
    shutil.copy2(MANIFEST,backup)
    known = {x['id'] for x in json.loads(MANIFEST.read_text(encoding='utf-8-sig'))['assets']}
    queue = [x for x in catalog() if 'uid://ambientcg_'+x[2].lower() not in known]
    count = 0; total = 0; failed = 0
    # One request at a time avoids placing heavy traffic on the provider.
    for item in queue:
        if shutil.disk_usage(ROOT).free < RESERVE + max(item[1]*2, 100*1024**2):
            print('reserve reached',shutil.disk_usage(ROOT).free,flush=True); break
        try:
            result = fetch(item)
            entry = save(result)
            commit([entry])
            count += 1; total += sum(len(x[1]) for x in result[3].values())
            if count == 1 or count % 10 == 0: print('imported',count,'bytes',total,'last',result[1],'free',shutil.disk_usage(ROOT).free,flush=True)
        except Exception as e:
            failed += 1; print('skip',item[2],repr(e),flush=True)
    print('FINISHED',count,total,'failures',failed,'free',shutil.disk_usage(ROOT).free,flush=True)

if __name__=='__main__': main()
