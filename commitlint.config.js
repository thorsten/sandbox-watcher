/** @type {import("@commitlint/types").UserConfig} */
export default {
  extends: ["@commitlint/config-conventional"],
  rules: {
    // Co-Authored-By trailers and URLs may exceed the default body line length.
    "body-max-line-length": [1, "always", 120],
    "footer-max-line-length": [0],
  },
};
