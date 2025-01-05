# Axum server with Opentelemetry & OTLP

Quickstart
```bash
RUST_LOG=info cargo watch -x 'run --package api'
```

- OTLP exporter
- Spans with some default opentelemetry tags
- Run jaeger OTLP container with
```bash
docker run --rm -d --name jaeger \
  -p 16686:16686 \
  -p 4317:4317 \
  -p 4318:4318 \
  jaegertracing/jaeger:2.1.0
```

n.b. not meant to be a reference or replacement for other libraries, but always had difficulties setting these up so a little experiment/reminder for myself
