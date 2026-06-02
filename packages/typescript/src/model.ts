export type ModelFormat = "plain" | "provider-qualified";

export interface ModelSelection {
  provider: string | undefined;
  name: string;
  original: string;
  format(format: ModelFormat): string;
}

export function resolveModel(provider: string | undefined, model: string | undefined): ModelSelection | undefined {
  if (!model) return undefined;

  const split = model.indexOf("/");
  const resolvedProvider = split >= 0 ? model.slice(0, split) : provider;
  const name = split >= 0 ? model.slice(split + 1) : model;

  return {
    provider: resolvedProvider,
    name,
    original: model,
    format(format: ModelFormat): string {
      if (format === "plain") return model;
      if (model.includes("/")) return model;
      return resolvedProvider ? `${resolvedProvider}/${name}` : name;
    },
  };
}
