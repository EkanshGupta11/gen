import json, os, subprocess, sys, tempfile, unittest
from app import tool

class ScientificTools(unittest.TestCase):
 def test_arithmetic(self): self.assertEqual(tool('arithmetic',{'a':4,'b':5})['value'],9)
 def test_zero_division(self):
  with self.assertRaisesRegex(ValueError,'zero'): tool('arithmetic',{'a':2,'b':0,'op':'divide'})
 def test_nonfinite_rejected(self):
  with self.assertRaisesRegex(ValueError,'finite'): tool('arithmetic',{'a':float('nan'),'b':1})
 def test_powerlaw_fit(self): self.assertAlmostEqual(tool('power_law',{'x':[1,2,4],'y':[2,8,32]})['exponent'],2)
 def test_singular_powerlaw_rejected(self):
  with self.assertRaisesRegex(ValueError,'singular'): tool('power_law',{'x':[2,2,2],'y':[1,2,3]})
 def test_projectile_assumptions(self): self.assertIn('vacuum',tool('projectile',{'speed':10,'angle_degrees':45})['assumptions'])
 def test_statistics(self): self.assertEqual(tool('statistics',{'values':[1,2,3]})['mean'],2)
 def test_unknown_tool(self):
  with self.assertRaisesRegex(ValueError,'Unknown'): tool('shell',{})
 def test_worker_requires_version(self):
  worker=os.path.join(os.path.dirname(__file__),'app.py')
  result=subprocess.run([sys.executable,worker,'/api/tool'],input=b'{"name":"arithmetic","args":{"a":1,"b":2}}',capture_output=True)
  self.assertEqual(result.returncode,2)
 def test_versioned_worker_tool_round_trip(self):
  worker=os.path.join(os.path.dirname(__file__),'app.py')
  result=subprocess.run([sys.executable,worker,'/api/tool'],input=json.dumps({'contract_version':'1.0','name':'arithmetic','args':{'a':20,'b':22}}).encode(),capture_output=True,check=True)
  self.assertEqual(json.loads(result.stdout)['result']['value'],42)
 def test_worker_exposes_computation_only(self):
  worker=os.path.join(os.path.dirname(__file__),'app.py')
  result=subprocess.run([sys.executable,worker,'/api/investigate'],input=b'{"contract_version":"1.0"}',capture_output=True)
  self.assertEqual(result.returncode,3)

if __name__=='__main__': unittest.main()
