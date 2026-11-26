# syntax=docker/dockerfile:1.7
#
# SOLPRINT frontend image: static build served by unprivileged nginx, which
# also reverse-proxies /api and /health to the backend so the browser only
# ever talks to one origin. Build context: ./frontend.

ARG NODE_IMAGE=node:24.22.3-trixie-slim
ARG NGINX_IMAGE=nginxinc/nginx-unprivileged:1.30.6-alpine3.24

# ---- build ----------------------------------------------------------------
FROM ${NODE_IMAGE} AS build
WORKDIR /app
ENV CI=true NPM_CONFIG_FUND=false NPM_CONFIG_AUDIT=false

COPY package.json package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci --ignore-scripts

COPY . .
# The internal /design-system showcase is excluded from container builds
# unless explicitly enabled.
ARG VITE_ENABLE_DESIGN_SYSTEM=false
ENV VITE_ENABLE_DESIGN_SYSTEM=${VITE_ENABLE_DESIGN_SYSTEM}
RUN npm run build

# ---- runtime --------------------------------------------------------------
FROM ${NGINX_IMAGE} AS runtime
COPY --from=build /app/dist /usr/share/nginx/html
COPY nginx.conf /etc/nginx/conf.d/default.conf
EXPOSE 8080
HEALTHCHECK --interval=10s --timeout=3s --start-period=10s --retries=3 \
    CMD ["wget", "-q", "-O", "/dev/null", "http://127.0.0.1:8080/healthz"]
