# Baseline pós-upgrade (set/2026) — Rust

Branch: `feature/upgrade-benchmark` (base: `main`)

## Procedimento
- `docker compose -f docker-compose.benchmark.yml -p bench-rust up -d` (postgres 18.6-alpine, nginx 1.30.4-alpine, tabela LOGGED)
- k6 `--summary-trend-stats "avg,min,med,max,p(90),p(95),p(99)"`, settle de 20s entre cenários pesados
- Logs brutos em `benchmark-results/baseline-rust-*.log`

## Resultados (todas as suítes verdes, exit=0)

| Cenário | p95 | p99 | RPS |
|---|---|---|---|
| smoke | 8.8ms | 12.25ms | 445 |
| post-heavy | 115.87ms | 151.97ms | 1636 |
| search-heavy | **4.79s** | **5s** | **26** |
| get-by-id-heavy | 1.6ms | 2.11ms | 4697 |
| mixed-rinha-like | 1.31s | 1.57s | 211 |

## Contrato
- contract-ko: 8/8 checks OK

## Notas de metodologia
- Scripts k6 normalizados entre os 3 repos (cópias do C++, threshold `http_req_failed{expected_response:true}`)

## Achados para otimização (Fase 3)
1. **`search-heavy` catastrófico (26 rps, p95 4.8s)**: `docker/postgres/init.sql` NÃO cria índice GIN em `searchable` (Go e C++ têm) → seq scan em toda busca. Fix: `CREATE INDEX CONCURRENTLY IF NOT EXISTS people_search_idx ON people USING GIN(searchable gin_trgm_ops);` + pg_prewarm
2. `post-heavy` p99 152ms já é o melhor dos 3
3. smoke p99 12ms — investigar depois (TCP_NODELAY no accept, resposta estática)
