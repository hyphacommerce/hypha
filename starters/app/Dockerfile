# The Node host as an image: `docker build -t my-hypha-app .` and run it with APP_URL set.
FROM node:24-slim
WORKDIR /app
ENV NODE_ENV=production PORT=8787
RUN corepack enable
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile --prod
COPY src ./src
USER node
EXPOSE 8787
CMD ["node", "src/node.ts"]
