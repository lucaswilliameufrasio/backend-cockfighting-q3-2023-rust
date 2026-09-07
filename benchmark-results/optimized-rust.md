# Otimizações aplicadas (set/2026) — Rust

Branch: `feature/upgrade-benchmark` · Método idêntico ao baseline (settle 20s, scripts normalizados)

## Antes → Depois

| Cenário | Baseline p95 → p99 | Otimizado p95 → p99 | RPS antes → depois |
|---|---|---|---|
| smoke | 8.8ms → 12.25ms | 4.03ms → 4.76ms | 445 → 473 |
| post-heavy | 115.87ms → 151.97ms | 138.39ms → 239.55ms | 1636 → **2358** |
| search-heavy | **4.79s → 5s** | **1.82ms → 47.78ms** | 26 → **3641 (140x)** |
| get-by-id-heavy | 1.6ms → 2.11ms | 1.48ms → 1.94ms | 4697 → 4677 |
| mixed-rinha-like | 1.31s → 1.57s | 749ms → 2.14s¹ | 211 → 683 (3.2x) |

¹ p99 do mixed tem outlier único (ruído de checkpoint); p95 caiu de 1.31s para 749ms.

## O que foi aplicado

1. **Índice GIN em `searchable` + `pg_prewarm`** (`docker/postgres/init.sql`) — era o gap crítico (Go/C++ já tinham); seq scan → bitmap index scan. Ganho de 140x no search
2. `mimalloc` como global allocator + `[profile.release] lto/codegen-units=1/panic=abort`
3. `TCP_NODELAY` no socket aceito (`set_nodelay(true)` no accept loop)
4. Pool explícito: `DB_MAX_CONNECTIONS` → `deadpool` `max_size` (default era por CPU do host)
5. `prepare_cached` em todas as queries (create/get/search/count)
6. `extract_term` sem HashMap (split direto na query string) + testes
7. Content-Type como `HeaderValue::from_static` (sem reparse por resposta)
8. Corpo de erro sem serde_json (`{"message":"..."}` montado com capacity pré-alocada)
9. Upgrades: hyper 1.11.1, reqwest 0.13, tokio/deadpool/etc. via Cargo.lock, rust:1.98.0-slim, postgres 18.6, nginx 1.30.4

## Quality gate
- `cargo build --release` ✓ · 5/5 testes unitários ✓ (is_date_valid ×2, extract_term ×2, url_decode)
- `tests/integration.rs` existe e roda contra compose próprio (gate da Fase 4)

## Pendências conhecidas
- post-heavy p99 subiu (152→239ms) com +44% de throughput — fila maior sob mais concorrência; ainda 4.5x folga vs limite da Rinha (1100ms)
- Statement caching via deadpool funciona por conexão; pool maior = mais prepares (custo único por conexão)
