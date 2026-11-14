import os, tempfile, unittest, json, uuid, subprocess, sys
from app import tool, investigate, db
class Tools(unittest.TestCase):
 def test_arithmetic(self): self.assertEqual(tool('arithmetic',{'a':4,'b':5})['value'],9)
 def test_bad_powerlaw(self):
  with self.assertRaises(ValueError): tool('power_law',{'x':[1,2],'y':[1]})
 def test_powerlaw(self): self.assertAlmostEqual(tool('power_law',{'x':[1,2,4],'y':[2,8,32]})['exponent'],2)
 def test_projectile_assumptions(self): self.assertIn('vacuum',tool('projectile',{'speed':10,'angle_degrees':45})['assumptions'])
 def test_invalid_question(self):
  with self.assertRaises(ValueError): investigate('tiny')
 def test_worker_process_returns_nonzero_on_bad_json(self):
  worker=os.path.join(os.path.dirname(__file__),'app.py')
  result=subprocess.run([sys.executable,worker,'/api/investigate'],input=b'{',capture_output=True)
  self.assertEqual(result.returncode,2)
  self.assertIn(b'"error"',result.stdout)
 def test_worker_process_returns_not_found_status(self):
  worker=os.path.join(os.path.dirname(__file__),'app.py')
  result=subprocess.run([sys.executable,worker,'/not-a-route'],input=b'{}',capture_output=True)
  self.assertEqual(result.returncode,3)
 def test_sqlite_survives_reopen(self):
  with tempfile.TemporaryDirectory() as folder:
   old=os.environ.get('DATABASE_PATH'); os.environ['DATABASE_PATH']=os.path.join(folder,'history.sqlite3')
   try:
    ident=str(uuid.uuid4()); c=db(); c.execute('INSERT INTO investigations VALUES(?,?,?,?)',(ident,'persist this question','now','{}')); c.commit(); c.close()
    c=db(); row=c.execute('SELECT question FROM investigations WHERE id=?',(ident,)).fetchone(); c.close()
    self.assertEqual(row[0],'persist this question')
   finally:
    if old is None: os.environ.pop('DATABASE_PATH',None)
    else: os.environ['DATABASE_PATH']=old
 def test_investigation_persists_across_worker_processes(self):
  worker=os.path.join(os.path.dirname(__file__),'app.py')
  with tempfile.TemporaryDirectory() as folder:
   env=os.environ.copy(); env['DATABASE_PATH']=os.path.join(folder,'restart.sqlite3')
   saved=subprocess.run([sys.executable,worker,'/api/investigate'],input=json.dumps({'question':'Explain how gravity affects projectile motion.'}).encode(),capture_output=True,env=env,check=True)
   investigation=json.loads(saved.stdout)
   history=subprocess.run([sys.executable,worker,'/api/history'],input=json.dumps({'q':'gravity'}).encode(),capture_output=True,env=env,check=True)
   records=json.loads(history.stdout)['items']
   self.assertEqual(records[0]['id'],investigation['id'])
 def test_offline_investigation_and_persisted_result(self):
  with tempfile.TemporaryDirectory() as folder:
   old=os.environ.get('DATABASE_PATH'); os.environ['DATABASE_PATH']=os.path.join(folder,'workflow.sqlite3')
   try:
    result=investigate('For a projectile launched at speed 20 m/s and angle 35 degrees, estimate ideal range.')
    self.assertEqual(result['mode'],'mock_offline'); self.assertIn('physics',result['plan']['specialists']); self.assertAlmostEqual(result['tools'][0]['result']['range_m'],38.32879,places=4)
    c=db(); saved=c.execute('SELECT result FROM investigations WHERE id=?',(result['id'],)).fetchone(); c.close()
    self.assertEqual(json.loads(saved[0])['id'],result['id'])
   finally:
    if old is None: os.environ.pop('DATABASE_PATH',None)
    else: os.environ['DATABASE_PATH']=old
 def test_optional_provider_requires_configuration(self):
  old=os.environ.get('MODEL_PROVIDER'); key=os.environ.pop('MODEL_API_KEY',None); os.environ['MODEL_PROVIDER']='openai_compatible'
  try:
   with self.assertRaisesRegex(ValueError,'requires MODEL_API_KEY'): investigate('Explain how gravity affects projectile motion.')
  finally:
   if old is None: os.environ.pop('MODEL_PROVIDER',None)
   else: os.environ['MODEL_PROVIDER']=old
   if key is not None: os.environ['MODEL_API_KEY']=key
if __name__=='__main__': unittest.main()
