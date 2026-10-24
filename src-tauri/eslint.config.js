import js from '@eslint/js';
import globals from 'globals';
import tseslint from 'typescript-eslint';
import reactHooks from 'eslint-plugin-react-hooks';

/**
 * 前端 ESLint 扁平配置（ESLint 9+ / flat config）。
 *
 * 规则取向：以「能发现真问题」为目标，不做风格洁癖。
 * 凡是与现有代码大量冲突的规则一律降级或关闭，不为了迁就规则去大改业务代码。
 */
export default tseslint.config(
  {
    ignores: ['dist/**', 'node_modules/**', 'src-tauri/target/**'],
  },

  js.configs.recommended,

  ...tseslint.configs.recommended,

  {
    files: ['**/*.{ts,tsx}'],
    languageOptions: {
      ecmaVersion: 2020,
      sourceType: 'module',
      globals: {
        ...globals.browser,
      },
    },
    plugins: {
      'react-hooks': reactHooks,
    },
    rules: {
      // React Hooks 规则：rules-of-hooks 是硬约束；
      // exhaustive-deps 提示缺失依赖，按需用 eslint-disable 局部豁免。
      ...reactHooks.configs.flat.recommended.rules,

      // 编译器版 hooks 规则（v7 新增）：与现有代码风格冲突较多，
      // 降级为 warn，避免淹没真正的问题。
      'react-hooks/static-components': 'warn',
      'react-hooks/immutability': 'warn',
      'react-hooks/purity': 'warn',
      'react-hooks/config': 'warn',
      'react-hooks/gating': 'warn',
      'react-hooks/preserve-manual-memoization': 'warn',

      // 下面两条与现有实现直接冲突，改造要动业务逻辑，先关掉：
      // - set-state-in-effect：数据拉取 effect 里同步 setLoading/setError 是有意为之；
      //   要消掉它得改成 React 19 的 use + Suspense，属于业务重构。
      // - refs：React 18 没有 useEffectEvent，「用 ref 持有最新导出函数」
      //   是当前实现规避重复注册监听器的办法。
      'react-hooks/set-state-in-effect': 'off',
      'react-hooks/refs': 'off',

      // TypeScript 已经做了一次类型检查，未使用变量交给 tsc 判断，
      // 这里只保留「声明了但完全没用到」的兜底。
      '@typescript-eslint/no-unused-vars': [
        'error',
        { argsIgnorePattern: '^_', varsIgnorePattern: '^_' },
      ],
      'no-unused-vars': 'off',

      'no-constant-binary-expression': 'error',
    },
    linterOptions: {
      // 未使用的禁用注释不报错：rules-of-hooks 的豁免按需保留，
      // 规则升级后失效也不应该变成噪音。
      reportUnusedDisableDirectives: 'off',
    },
  },

  {
    files: ['vite.config.ts'],
    languageOptions: {
      globals: {
        ...globals.node,
      },
    },
  },
);
