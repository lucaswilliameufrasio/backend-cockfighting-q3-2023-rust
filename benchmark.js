import http from 'k6/http';
import { check, sleep } from 'k6';

export const options = {
  stages: [
    { duration: '10s', target: 50 },
    { duration: '30s', target: 200 },
    { duration: '10s', target: 500 },
    { duration: '30s', target: 500 },
    { duration: '10s', target: 0 },
  ],
  thresholds: {
    http_req_duration: ['p(95)<500', 'p(99)<1000'],
    http_req_failed: ['rate<0.01'],
  },
};

const BASE_URL = 'http://localhost:8080';

export default function () {
  // POST /pessoas
  const payload = JSON.stringify({
    apelido: `user${__VU}_${__ITER}`,
    nome: 'User',
    nascimento: '1990-01-01',
    stack: ['Go', 'Rust'],
  });
  const createResp = http.post(`${BASE_URL}/pessoas`, payload, {
    headers: { 'Content-Type': 'application/json' },
    tags: { name: 'create-person' },
  });
  check(createResp, { 'create status 201': (r) => r.status === 201 });

  if (createResp.status === 201) {
    const id = createResp.json('id');
    // GET /pessoas/{id}
    const getResp = http.get(`${BASE_URL}/pessoas/${id}`, {
      tags: { name: 'get-person' },
    });
    check(getResp, { 'get status 200': (r) => r.status === 200 });
  }

  // GET /pessoas?t=termo
  const searchResp = http.get(`${BASE_URL}/pessoas?t=user`, {
    tags: { name: 'search' },
  });
  check(searchResp, { 'search status 200': (r) => r.status === 200 });

  // GET /contagem-pessoas
  const countResp = http.get(`${BASE_URL}/contagem-pessoas`, {
    tags: { name: 'count' },
  });
  check(countResp, { 'count status 200': (r) => r.status === 200 });

  sleep(0.1);
}
