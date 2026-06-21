"""Quick smoke test for the Flask app."""
import os
import sys
import re

# 让测试可以找到 app.py
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

import app

client = app.app.test_client()

# 1. GET /login should succeed
r = client.get('/login')
print('GET /login:', r.status_code, 'len=', len(r.data))
assert r.status_code == 200

# 2. POST /login without CSRF should fail (403)
r = client.post('/login', data={'username': 'test', 'password': 'test'})
print('POST /login without CSRF:', r.status_code)
assert r.status_code == 403

# 3. With session, GET /login to get CSRF token
r = client.get('/login')
html = r.data.decode('utf-8')
m = re.search(r'name="_csrf_token" value="([^"]+)"', html)
assert m, 'CSRF token not found in login page'
token = m.group(1)
print('CSRF token found:', token[:12] + '...')

# 4. POST with CSRF + wrong creds -> 200 (re-renders with error)
r = client.post('/login', data={'username': 'test', 'password': 'test', '_csrf_token': token})
print('POST /login with CSRF (wrong creds):', r.status_code)
assert r.status_code == 200

print('\nAll smoke tests passed!')
