# Legacy Zabor / эталон старого клиента

`zabor-electron-c5d07348.zip` permanently preserves the exact tracked Electron/React
application at commit `c5d07348880555a3a22b781d6b810a01e4baff5d`, including source,
resources, build configuration, tests and legal documents. `manifest.json` records
every Git blob, byte length and SHA-256. It contains no user profile, `.env`,
`node_modules` or compiler cache.

Архив сохраняет старый Electron/React-клиент как эталон и историческую копию.
Исходники и ресурсы проверяются побайтно по Git; оригинальный проект пользователя
не изменён. Архив не входит в нативную поставку и не является её рантаймом.

Verify / проверка:

```cmd
cmd.exe /c python -B scripts/native/archive-legacy.py check
```

Extract into a separate empty directory to inspect or rebuild the old client. The
archive retains the historical commands; use an isolated dependency installation,
never the shared `node_modules` junction. Do not run its release scripts against
the native release branch.

Until full native functional and visual acceptance, active reference paths in the
repository remain in place to avoid breaking existing compatibility tests. Their
eventual removal must preserve this archive and the pinned reference inventories.

До полной функциональной и визуальной приёмки native-клиента действующие пути
эталона в репозитории сохраняются. Это намеренная защита проверок совместимости,
а не объявление переноса завершённым.
