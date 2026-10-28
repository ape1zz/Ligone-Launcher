# OAuth Ely.by в Ligone Launcher

## Как работает

1. Лаунчер открывает в браузере страницу авторизации `https://account.ely.by/oauth2/v1?...` (authorization code flow).
2. Параллельно поднимается локальный callback-сервер на `http://localhost:8921/ely/callback`.
3. Браузер возвращает `code` и `state` (проверяется на подделку). Лаунчер обменивает код на токены (`POST /api/oauth2/v1/token`, `grant_type=authorization_code`).
4. По `access_token` запрашивается профиль (`GET /api/account/v1/info`) — username и UUID попадают в аккаунт.
5. Запрошены скоупы `account_info offline_access minecraft_server_session`.Благодаря `offline_access` сохраняется `refresh_token` — им можно обновить `access_token` командой `auth_ely_refresh`.
6. Аккаунты хранятся в `<домашняя папка лаунчера>/auth/accounts.json` (Windows: `%APPDATA%\.ligone\auth\accounts.json`), как и офлайн-аккаунты, но токены Ely.by там не лежат: в файле остаётся безтокенный маркер `ely: {}`, а полные токены — в OS keyring (Windows Credential Manager, service `com.ligone.launcher`, user `ely@<username>`). При загрузке токены восстанавливаются из keyring; при первом чтении inline-токены из старых файлов переносятся в keyring автоматически. Домашняя папка лаунчера — `%APPDATA%\.ligone`; при первом запуске данные из старого `%APPDATA%\com.ligone.launcher\auth\` переносятся туда автоматически.
7. После успешного входа браузер остаётся с открытой вкладкой — Ely.by перенаправляет её на локальный callback-сервер, который отвечает страницей «Авторизация успешна…». Сайт не «возвращает» вас в окно приложения автоматически: лаунчер продолжает вход сам, в этот же момент, а вкладку браузера можно закрыть.

## Настройка (обязательно перед использованием)

1. Зарегистрируйте приложение на https://account.ely.by/dev/applications/new — тип **Website**.
2. В поле Redirect URI укажите ровно `http://localhost:1420/ely/callback` (это порт dev-сервера Vite; в колбэк-сервер лаунчера запрос попадает через прокси Vite, bind на `127.0.0.1:8921`). Для RELEASE-сборки, где Vite-прокси отсутствует, зарегистрируйте/задайте напрямую `http://localhost:8921/ely/callback` — переменной окружения `ELYBY_REDIRECT_URI` или константы `DEFAULT_REDIRECT_URI` в `ely.rs`.
3. Скопируйте `clientId` и `clientSecret` в файл `.env` в домашней папке лаунчера (`%APPDATA%\.ligone\.env`) либо задайте переменные окружения `ELYBY_CLIENT_ID` и `ELYBY_CLIENT_SECRET`. Переменные из окружения имеют приоритет над `.env`; секреты нигде не хардкодятся (при отсутствии обоих OAuth- и парольный вход завершатся ошибкой «не настроен»).
4. Порт колбэк-сервера (`callback_bind_port` в `ely.rs`) автоматически берётся из redirect URI; URI с портом 1420 в dev работает только через прокси Vite.

## Команды

- `auth_login_ely` — полный вход (браузер → колбэк → токены → профиль → сохранение); пока команда ждёт колбэк, её можно отменить командой `auth_cancel_ely` (ставит флаг отмены, команда вернёт ошибку `cancelled` — диалог выбора способа в UI показывает экран ожидания с кнопкой возврата).
- `auth_ely_refresh` — обновить `access_token` по `refresh_token` текущего аккаунта (при успехе возвращает обновлённый аккаунт).

Ошибки: `oauth` (проблемы на этапе авторизации) и `ely_api` (проблемы токен-эндпоинта/профиля) — те же tagged-enum, что и остальные ошибки `auth`.

## Парольный вход (запасной вариант)

Соседний путь, как в legacy-лаунчерах: вход по E-mail/нику и паролю прямо в окне лаунчера, без браузера. Используется authserver-протокол Ely.by (симуляция Mojang-авторизации): `POST https://authserver.ely.by/auth/authenticate` (`username`, `password`, `clientToken`) → `accessToken` + `selectedProfile` (UUID без дефисов; нормализуется). Команда — `auth_login_ely_password`.

- OAuth (кнопка «Ely.by» в лаунчере) остаётся рекомендуемым способом; парольный вход доступен в диалоге выбора способа.
- Пароль нигде не сохраняется и не логируется — в `accounts.json` для Ely-аккаунта нет ни пароля, ни токенов (токены — в keyring); поле `clientToken` лаунчера генерируется один раз и лежит в `%APPDATA%\.ligone\auth\client_token.json`.
- Токен обновляется через `POST /auth/refresh` (`accessToken` + `clientToken`) той же командой `auth_ely_refresh` (у legacy-аккаунта вместо `refreshToken` используется `clientToken`; в keyring такой аккаунт хранится с `clientToken` и нулевым `accessTokenExpiresAt`).
- Двухфакторные аккаунты: сервер вернёт «Account protected with two factor auth» — лаунчер покажет подсказку войти через OAuth (специальный метод вроде `password:ТОТП` пока не реализован).
