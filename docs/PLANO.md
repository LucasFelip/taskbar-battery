# taskbar-battery — plano de implementação

Monitor de bateria de periféricos na bandeja do Windows 11, com consumo de RAM e CPU mínimos.

## 1. Decisões já tomadas

| Tema | Decisão |
|---|---|
| Linguagem | Rust + Win32 puro (`windows-sys`, sem `windows` crate completo, sem async runtime) |
| Interface | Um ícone de bandeja por dispositivo, no padrão visual do Windows 11 (glifos de bateria da fonte Segoe Fluent Icons) |
| Sem bateria informada | Mostrar ícone com `?` (ver regra de filtragem na seção 4) |
| Bateria baixa | Só muda o ícone para vermelho (< 20%). Sem toast |
| Leitura | A cada 10 min + eventos de conectar/desconectar |
| Celular | Samsung A31 por cabo (USB). ADB é o plano B (ver seção 3.4) |
| Entrega | `.exe` portátil + opção no menu "Iniciar com o Windows" (chave `HKCU\...\Run`) |
| Escopo de dispositivos | Todos os que estiverem conectados por qualquer meio (Bluetooth, cabo, receptor 2.4 GHz) |

## 2. Metas de consumo (critério de aceite)

- RAM privada em repouso: **≤ 2 MB** (Working Set ≤ 4 MB).
- CPU em repouso: **0,0%**. O processo dorme em `GetMessageW`, sem loop de polling.
- `.exe` ≤ 400 KB.
- Zero threads em repouso além da principal. Sem processos filhos permanentes.

Como chegar lá:
- `opt-level="z"`, `lto="fat"`, `codegen-units=1`, `panic="abort"`, `strip=true`.
- Sem `std::fmt` pesado no caminho quente. Buffers fixos na pilha (`[u16; N]`).
- Após cada ciclo de leitura, chamar `EmptyWorkingSet` para devolver páginas ao sistema.
- Ícones desenhados uma vez por (glifo, cor) e reaproveitados do cache. Sem recriar `HICON` a cada ciclo.

## 3. Arquitetura

Um único processo, uma janela oculta (message-only) que recebe tudo:

```
WM_TIMER (10 min) ──┐
WM_DEVICECHANGE ────┼─► refresh() ─► [fontes] ─► lista de Device ─► diff ─► Shell_NotifyIconW
CM_Register_Notification (debounce 1,5 s)
WM_TASKBARCREATED ──► recria ícones (explorer reiniciou)
```

Modelo de dados mínimo:

```rust
struct Device { id: u64 /*hash do instance id*/, nome: [u16; 64], pct: Option<u8>, carregando: bool, fonte: Fonte }
```

Cada ícone de bandeja usa um **GUID derivado do `id`** (`NIF_GUID`). Assim o Windows lembra a posição e o estado "fixado" do ícone entre execuções.

### 3.1 Fonte Bluetooth (Classic e LE)
- Enumerar com `CM_Get_Device_ID_List` (filtro `BTHENUM`, `BTHLE`) e ler a propriedade `DEVPKEY_Bluetooth_Battery` `{104EA319-6EE2-4701-BD47-8DDBF425BBE5},2` com `CM_Get_DevNode_PropertyW`.
- Sem WinRT e sem abrir conexão GATT. É só leitura de propriedade, que o Windows já mantém atualizada.
- Fones que reportam bateria por HFP aparecem pela mesma propriedade.

### 3.2 Fonte HID (mouse e teclado por cabo ou receptor)
- Enumerar interfaces HID com SetupAPI (`GUID_DEVINTERFACE_HID`) e abrir com `CreateFileW` sem acesso de leitura/escrita (só consulta de atributos).
- Ler o descritor de report (`HidP_GetCaps`, `HidP_GetValueCaps`) e procurar usage page `0x06` / usage `0x20` (Battery Strength) ou page `0x85` (Battery System). Se existir, ler com `HidD_GetFeatureReport` ou `HidD_GetInputReport`.
- Dispositivos de fabricantes com protocolo próprio (Logitech HID++, Razer, Keychron via Launcher) ficam como **drivers opcionais** da fase 3, só se os modelos reais exigirem.

### 3.3 Fonte celular via WPD/MTP (reserva; REVISADO na fase 0: valor defasado)
- O celular já aparece como dispositivo portátil (com o nome dado pelo usuário). O WPD define a propriedade `WPD_DEVICE_POWER_LEVEL`, e muitos Android a preenchem com o percentual da bateria.
- Vantagem: nenhuma dependência extra e nenhum processo ADB.
- Risco: o Samsung pode não preencher a propriedade, ou só preencher com a tela desbloqueada e o modo MTP ativo. **Validar na fase 0.**

### 3.4 Fonte celular via ADB (PRINCIPAL após a fase 0: WPD deu 97, ADB deu 100)
- Falar com o servidor ADB já em execução em `127.0.0.1:5037` por socket TCP, com o protocolo `host:transport:<serial>` + `shell:dumpsys battery`. Sem criar processo.
- Se o servidor ADB não estiver rodando, **não iniciá-lo** (o daemon dele usa 10 a 20 MB). Mostrar o celular com `?` e uma dica no tooltip.
- Alternativa só se for necessário: spawn de `adb.exe` sob demanda, uma vez por ciclo, sem manter o daemon.

### 3.5 Gatilhos de atualização
- `CM_Register_Notification` para interfaces de dispositivo (HID, Bluetooth, USB, WPD): chegada e remoção, com debounce de 1,5 s para juntar rajadas.
- Timer de 10 min só para reler o percentual.
- Dispositivo que some: remove o ícone (`NIM_DELETE`) no ciclo seguinte à remoção.

## 4. Regra de filtragem

Mostrar `?` para todo dispositivo conectado sem bateria encheria a bandeja (o PC tem dezenas de nós HID e virtuais). A regra proposta para decidir o que vira ícone:

1. Agrupar os nós por **dispositivo físico** (pai USB ou dispositivo Bluetooth), um ícone por grupo.
2. Considerar só dispositivos físicos de uso do usuário: mouse, teclado, controle, fone/áudio Bluetooth, celular.
3. Excluir: raiz/hub USB, `ROOT\`, `SWD\`, `LGHUBDEVICE`, `USBip`, câmeras e microfones virtuais (Iriun), monitores, saídas de áudio HDMI/DisplayPort.
4. Menu de contexto de cada ícone: "Ocultar este dispositivo". A lista de ocultos fica em `%APPDATA%\taskbar-battery\config.ini`.

## 5. Interface (ícones no padrão Windows 11)

- Glifos da fonte **Segoe Fluent Icons** (disponível no Windows 11): `Battery0` a `Battery10` e as variantes de carregamento. Os códigos exatos serão **validados na fase 1** contra a fonte instalada, em vez de assumidos.
- Renderização: bitmap 16×16 (escalado por DPI: 16/20/24/32) com GDI, cor do texto seguindo o tema claro/escuro (`AppsUseLightTheme`). Abaixo de 20%, glifo vermelho.
- Sem bateria conhecida: glifo de bateria vazia com `?`.
- Tooltip (`NIF_TIP`): `Nome — 73%` / `Nome — carregando` / `Nome — bateria indisponível`.
- Menu de contexto (nativo, `TrackPopupMenu`): Atualizar agora, Ocultar este dispositivo, Iniciar com o Windows (marcável), Sair.
- Limitação conhecida: o Windows 11 coloca ícones novos no overflow ("^"). O usuário precisa arrastar cada um para a barra uma única vez. O uso de GUID fixo faz o Windows lembrar a escolha.

## 6. Fases

**Fase 0 — Descoberta (sem UI), ~1 sessão**
- Script/binário de linha de comando que lista cada dispositivo físico e o que ele expõe: PnP battery, HID battery usage, WPD power level.
- Identificar de fato o mouse (VID 249A:5C2F) e o teclado (VID 05AC:024F).
- Testar o fone QCY ligado e o celular via WPD e via ADB.
- Saída: tabela "dispositivo → fonte que funciona". Define o que entra no MVP.

**Fase 1 — Núcleo + 1 ícone, ~1 a 2 sessões**
- Projeto Cargo, janela oculta, `Shell_NotifyIconW`, desenho do glifo, timer e `EmptyWorkingSet`.
- Fonte Bluetooth, com ícone real e cor vermelha abaixo de 20%.

**Fase 2 — Multi-dispositivo e eventos, ~1 a 2 sessões**
- Fontes HID e celular (WPD, depois ADB se preciso).
- Agrupamento, filtragem, GUID por dispositivo, notificação de conectar/desconectar com debounce.
- `?` para dispositivos físicos sem bateria.

**Fase 3 — Acabamento, ~1 sessão**
- Menu de contexto, `config.ini`, iniciar com o Windows, tema claro/escuro, DPI.
- Drivers opcionais por fabricante, se a fase 0 mostrar que são necessários.

## 7. Verificação

- RAM/CPU: `Get-Process taskbar-battery` (PrivateMemorySize64, WorkingSet64) após 1 min e após 1 h; tempo de CPU acumulado após 1 h (deve ficar na casa de poucos milissegundos).
- Eventos: ligar/desligar fone e receptor e conferir que o ícone aparece e some em até ~2 s.
- Explorer: matar e reiniciar o `explorer.exe` e conferir que os ícones voltam (`TaskbarCreated`).
- Compilação e checagem de tamanho: `cargo build --release` e conferir o tamanho do `.exe`.
- Ícones e overflow: conferência visual do usuário na tela, já que não há como automatizar isso de forma confiável.

## 8. Riscos

| Risco | Mitigação |
|---|---|
| Mouse 2.4 GHz genérico (249A:5C2F) provavelmente não reporta bateria | Mostrar `?`; avaliar protocolo do fabricante na fase 0 |
| Keychron por cabo não tem bateria; por Bluetooth, sim | Detectar o modo; por cabo, mostrar `?` ou ocultar |
| Samsung pode não expor bateria por MTP | Plano B via ADB (3.4) |
| Ícones novos caem no overflow | Documentar; GUID fixo preserva a escolha |
| Códigos dos glifos da Segoe Fluent mudarem | Validar contra a fonte instalada na fase 1 |

## 9. Pendências (perguntas em aberto)

1. Modelo e marca exatos do **mouse** e do **teclado**, e em qual modo cada um é usado (cabo, 2.4 GHz, Bluetooth). Se não souber, a fase 0 tenta identificar.
2. O ícone deve mostrar também o **número** (por exemplo "73") além do glifo? Em 16 px o número fica apertado; a proposta é glifo no ícone e número no tooltip.
3. O **fone QCY** (e outros pareados) fica ligado quando for testar? Será preciso ligá-lo para a fase 0.

> **Revisão pós-fase 0:** ADB passa a ser a fonte principal do celular. Estratégia: a cada leitura, localizar `adb.exe` (config, PATH, cópia do Iriun), executar `adb shell dumpsys battery`, e `adb kill-server` se o servidor foi iniciado por nós. Custo: ~1 s de CPU a cada 10 min, sem daemon residente. Ver FASE0.md.


