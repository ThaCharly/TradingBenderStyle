# Estrategia de ramas

Trunk-based, un solo usuario, muchos agentes.

## Ramas

| Rama | Uso |
|---|---|
| `main` | Tronco. Siempre verde. Es la rama por defecto del repo. Nadie pushea directo. |
| `issue-<n>-<slug>` | Una rama por issue, creada desde `main` actualizado. Ej.: `issue-32-grabador-binance`. |
| `spike/<n>-<slug>` | Spikes (`tipo:spike`). El código del prototipo vive en `spikes/` y puede no cumplir todos los estándares; lo que sí se mergea es el documento con la decisión. |
| `ccr-*` | Ramas de sesiones de planificación con Claude. Se mergean a `main` por PR y se descartan. |

## Flujo

1. Partir de `main` actualizado: `git switch main && git pull && git switch -c issue-<n>-<slug>`.
2. Commits chicos y descriptivos.
3. PR hacia `main` con `Closes #<n>` y la lista de criterios de aceptación tildada, cada uno
   con el test o comando que lo verifica.
4. CI verde obligatoria.
5. **Squash merge**: un commit por issue en `main`.
6. Si `main` avanzó mientras tanto: `git merge main` en la rama del issue (no rebase de
   ramas ya publicadas).
7. Borrar la rama al mergear.

## Protección de `main` (configurar en GitHub → Settings → Branches)

- Requerir PR antes de mergear.
- Requerir que pasen los checks de CI: `docs`, `rust`, `ts` y `buf` (workflow
  `ci`).
- Requerir que la rama esté al día con `main`.
- Prohibir force-push y borrado.

## Versionado y releases

Hasta que haya algo desplegado en la mini-PC no hay releases. Después: tags `vAAAA.MM.DD`
desde `main`; el perfil de despliegue registra qué tag corre en cada nodo.

## Reglas para agentes

Las de [AGENTES.md](AGENTES.md). Un agente nunca pushea a `main` ni a la rama de otro issue.
