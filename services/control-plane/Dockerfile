# syntax=docker/dockerfile:1
FROM oven/bun:1.4.2-alpine AS dependencies
WORKDIR /app
COPY package.json bun.lock ./
RUN bun install --production --frozen-lockfile

FROM oven/bun:1.4.2-alpine AS runtime
WORKDIR /app
ENV NODE_ENV=production PORT=3000
COPY --from=dependencies /app/node_modules ./node_modules
COPY package.json bun.lock tsconfig.json ./
COPY LICENSE THIRD_PARTY_NOTICES.md ./
COPY src ./src
COPY public ./public
COPY scripts ./scripts
COPY migrations ./migrations
USER bun
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
  CMD ["bun", "-e", "const r=await fetch('http://127.0.0.1:3000/health');if(!r.ok)process.exit(1)"]
CMD ["bun", "run", "start"]
