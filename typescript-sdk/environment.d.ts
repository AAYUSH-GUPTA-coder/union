interface EnvironmentVariables {
  readonly NODE_ENV: "development" | "production" | "test"
  readonly DRY_RUN: string
  readonly HUBBLE_URL: string
}

declare module "bun" {
  interface Env extends EnvironmentVariables {}
}

declare namespace NodeJS {
  interface ProcessEnv extends EnvironmentVariables {}
}
