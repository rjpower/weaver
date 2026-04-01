const path = require('path');
const { HtmlRspackPlugin } = require('@rspack/core');
const { VueLoaderPlugin } = require('rspack-vue-loader');

module.exports = {
  entry: './src/main.ts',
  output: {
    path: path.resolve(__dirname, '../static/dist'),
    filename: 'app.[contenthash:8].js',
    clean: true,
  },
  resolve: {
    extensions: ['.ts', '.js', '.vue'],
  },
  plugins: [
    new VueLoaderPlugin(),
    new HtmlRspackPlugin({ template: './src/index.html', filename: 'index.html' }),
  ],
  module: {
    rules: [
      { test: /\.vue$/, loader: 'rspack-vue-loader', options: { experimentalInlineMatchResource: true } },
      { test: /\.ts$/, loader: 'builtin:swc-loader', options: { jsc: { parser: { syntax: 'typescript' } } }, type: 'javascript/auto' },
      { test: /\.css$/, use: ['postcss-loader'], type: 'css' },
    ],
  },
  experiments: {
    css: true,
  },
  mode: 'production',
};
