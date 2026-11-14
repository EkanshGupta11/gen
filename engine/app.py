"""Versioned scientific workflow worker. Standard library only."""
import json, math, os, sqlite3, sys, time, uuid, statistics, urllib.request, urllib.error
from datetime import datetime, timezone
DB=os.getenv('DATABASE_PATH','data/neurogenesis.sqlite3')
class ProviderError(Exception): pass
class ApiError(Exception): pass
ROLES={"mathematics":"Check mathematical structure and quantitative claims.","physics":"Assess physical principles, assumptions, and model limits.","chemistry":"Assess chemical concepts and reaction reasoning.","biology":"Assess biological mechanisms and evidence needs.","medicinal_science":"Assess pharmacology and translational drug research context."}
def db():
 database_path=os.getenv('DATABASE_PATH',DB)
 os.makedirs(os.path.dirname(database_path) or '.',exist_ok=True); c=sqlite3.connect(database_path); c.execute('CREATE TABLE IF NOT EXISTS investigations(id TEXT PRIMARY KEY, question TEXT, created_at TEXT, result TEXT)'); c.commit(); return c
def tool(name,args):
 if name=='arithmetic':
  a=float(args['a']); b=float(args['b']); op=args.get('op','add'); v={"add":a+b,"subtract":a-b,"multiply":a*b,"divide":a/b if b else None}.get(op)
  if v is None: raise ValueError('Unsupported operation or division by zero')
  return {"value":v,"operation":op,"epistemic_status":"calculated","method":"IEEE-754 floating-point arithmetic"}
 if name=='power_law':
  xs=list(map(float,args['x'])); ys=list(map(float,args['y']))
  if len(xs)!=len(ys) or len(xs)<3 or any(x<=0 for x in xs+ys): raise ValueError('Power-law fit requires at least 3 paired positive values')
  lx=list(map(math.log,xs)); ly=list(map(math.log,ys)); mx=statistics.mean(lx); my=statistics.mean(ly); slope=sum((x-mx)*(y-my) for x,y in zip(lx,ly))/sum((x-mx)**2 for x in lx); intercept=my-slope*mx; pred=[intercept+slope*x for x in lx]; r2=1-sum((y-p)**2 for y,p in zip(ly,pred))/sum((y-my)**2 for y in ly)
  return {"coefficient":math.exp(intercept),"exponent":slope,"r_squared_log_space":r2,"epistemic_status":"calculated","method":"ordinary least squares in log-log space"}
 if name=='polynomial':
  co=list(map(float,args['coefficients'])); x=float(args.get('x',0)); y=sum(c*x**(len(co)-i-1) for i,c in enumerate(co)); return {"value":y,"epistemic_status":"calculated","method":"polynomial evaluation; coefficients highest degree first"}
 if name=='projectile':
  v=float(args['speed']); deg=float(args['angle_degrees']); g=float(args.get('gravity',9.80665))
  if v<0 or g<=0: raise ValueError('Speed must be nonnegative and gravity positive')
  rad=math.radians(deg); return {"range_m":v*v*math.sin(2*rad)/g,"flight_time_s":2*v*math.sin(rad)/g,"max_height_m":v*v*math.sin(rad)**2/(2*g),"assumptions":["vacuum","level launch and landing","constant gravity"],"epistemic_status":"calculated","method":"ideal projectile equations; not experimental validation"}
 if name=='statistics':
  xs=list(map(float,args['values']))
  if len(xs)<2: raise ValueError('At least two numeric values required')
  return {"count":len(xs),"mean":statistics.mean(xs),"sample_stdev":statistics.stdev(xs),"min":min(xs),"max":max(xs),"epistemic_status":"calculated","method":"Python standard-library descriptive statistics"}
 raise ValueError('Unknown scientific tool')
def investigate(q):
 if not isinstance(q,str) or len(q.strip())<8 or len(q)>4000: raise ValueError('Question must be 8–4000 characters')
 q=q.strip(); low=q.lower(); selected=['mathematics']
 for key,terms in {'physics':['physics','force','motion','energy','gravity','oscillat','differential equation','projectile'],'chemistry':['chemical','reaction','molecule','chemistry'],'biology':['biology','cell','gene','protein','biological'],'medicinal_science':['drug','medicinal','pharmac','therapeutic']}.items():
  if any(t in low for t in terms): selected.append(key)
 plan={"question":q,"specialists":selected,"steps":["Classify relevant disciplines","Run deterministic calculation when a supported tool applies","Mark claims by evidence and verification status","Persist the investigation"]}
 result={"contract_version":"1.0","id":str(uuid.uuid4()),"created_at":datetime.now(timezone.utc).isoformat(),"mode":"mock_offline","plan":plan,"activity":[],"findings":[],"sources":[],"tools":[],"verification":[],"synthesis":"Offline workflow demonstration. No scholarly sources or hosted model were queried; discipline-specific analysis is a capability preview, not a scientific conclusion.","epistemic_status":"hypothesis","limitations":["Mock mode does not provide external scientific evidence.","No model-generated result is represented as research fact."]}
 for role in selected: result['activity'].append({"specialist":role,"instruction":ROLES[role],"status":"role_selected","output":"Role selected only; no analysis ran in mock mode."})
 if os.getenv('MODEL_PROVIDER','mock')!='mock':
  key=os.getenv('MODEL_API_KEY'); base=os.getenv('MODEL_BASE_URL','').rstrip('/'); model=os.getenv('MODEL_NAME')
  if not key or not base or not model: raise ValueError('Hosted provider requires MODEL_API_KEY, MODEL_BASE_URL, and MODEL_NAME')
  prompt={"question":q,"specialists":[{"role":r,"instructions":ROLES[r]} for r in selected],"requirements":"Return strict JSON with an analyses array of {specialist, analysis, claims:[{text, epistemic_status, provenance}]}, and synthesis. Do not invent sources or claim verification. State when evidence is unavailable."}
  request=urllib.request.Request(base+'/chat/completions',data=json.dumps({"model":model,"messages":[{"role":"system","content":"You are a scientific research assistant. Distinguish evidence from hypothesis; never fabricate citations. Return only JSON."},{"role":"user","content":json.dumps(prompt)}],"max_tokens":int(os.getenv('MODEL_MAX_TOKENS','1200'))}).encode(),headers={"Authorization":"Bearer "+key,"Content-Type":"application/json"},method='POST')
  try:
   with urllib.request.urlopen(request,timeout=int(os.getenv('MODEL_TIMEOUT_SECONDS','30'))) as response: completion=json.loads(response.read())
   content=completion['choices'][0]['message']['content']; parsed=json.loads(content)
   result['mode']='hosted_model'; result['synthesis']=parsed.get('synthesis','Model returned no synthesis.'); result['model']=model
   by_role={item.get('specialist'):item for item in parsed.get('analyses',[]) if isinstance(item,dict)}
   for activity in result['activity']:
    item=by_role.get(activity['specialist'])
    if item:
     activity['status']='model_generated'; activity['output']=str(item.get('analysis',''))[:5000]
   result['findings']=parsed.get('analyses',[])
  except (urllib.error.URLError,TimeoutError,KeyError,IndexError,json.JSONDecodeError) as e: raise ProviderError('Hosted model request failed or returned malformed structured output: '+str(e))
 if 'projectile' in low and 'speed' in low:
  import re
  nums=[float(x) for x in re.findall(r'-?\d+(?:\.\d+)?',q)]
  if len(nums)>=2:
   try: result['tools'].append({"tool":"projectile","result":tool('projectile',{"speed":nums[0],"angle_degrees":nums[1]})})
   except ValueError: pass
 result['verification']=[{"claim":"Any generated numerical result","status":"calculated" if result['tools'] else "uncertain","method":"Deterministic local tool; source-backed scientific claims are not available in offline mock mode."}]
 c=db(); c.execute('INSERT INTO investigations VALUES(?,?,?,?)',(result['id'],q,result['created_at'],json.dumps(result))); c.commit(); c.close(); return result
def main(path):
 raw=sys.stdin.read(); payload=json.loads(raw or '{}')
 if path in ('/api/investigate','/api/investigations') and payload: out=investigate(payload.get('question'))
 elif path.startswith('/api/history'):
  c=db(); rows=c.execute('SELECT id,question,created_at,result FROM investigations ORDER BY created_at DESC LIMIT 100').fetchall(); c.close(); query=payload.get('q','').lower(); out={"items":[{"id":r[0],"question":r[1],"created_at":r[2],"result":json.loads(r[3])} for r in rows if query in r[1].lower()]}
 elif path=='/api/tool': out={"result":tool(payload.get('name'),payload.get('args',{}))}
 elif path=='/api/settings': out={"model_provider":os.getenv('MODEL_PROVIDER','mock'),"memory_provider":os.getenv('MEMORY_PROVIDER','sqlite'),"hosted_ready":bool(os.getenv('MODEL_API_KEY'))}
 else: raise ApiError('Unknown API route')
 print(json.dumps(out,allow_nan=False))
if __name__=='__main__':
 try: main(sys.argv[1])
 except ProviderError as e: print(json.dumps({"error":str(e)})); sys.exit(4)
 except (ValueError,KeyError,TypeError,json.JSONDecodeError) as e: print(json.dumps({"error":str(e)})); sys.exit(2)
 except ApiError as e: print(json.dumps({"error":str(e)})); sys.exit(3)
 except Exception as e: print(json.dumps({"error":"Scientific worker failed unexpectedly: "+str(e)})); sys.exit(1)
