import type { en } from "./en";

/** Shape every language must fill completely. */
export type Dictionary = typeof en;
export type Key = keyof Dictionary;
export type PluralForms = { one: string; other: string };
/** Keys whose value has plural forms; `t` needs `n` for them. */
export type PluralKey = { [K in Key]: Dictionary[K] extends string ? never : K }[Key];
export type PlainKey = Exclude<Key, PluralKey>;
export type Params = Record<string, string | number>;
