# Taskbar Battery

Mostra a bateria dos seus periféricos (mouse, teclado, fone, controle, celular Android) como **ícones na bandeja do Windows 11**, com consumo mínimo de RAM e CPU.

- **Leve:** um único `.exe` de ~400 KB em Rust, sem instalador obrigatório e sem o Visual C++ Redistributable. Em repouso fica com ~0,1 a 0,6 MB de working set (≈3 MB commit) e 0% de CPU: o processo dorme esperando mensagens do Windows.
- **Privado:** sem rede, sem telemetria, sem contas. Tudo fica no seu PC.
- **Nativo:** ícones de bandeja do Windows (API pública), um por dispositivo. Três estilos: bateria, número, ou tipo do dispositivo + barra de nível. Vermelho abaixo de 20%. Segue o tema claro/escuro e o DPI.
- **Automático:** detecta o que está conectado, sem configuração. Atualiza ao conectar/desconectar, ao acordar do sleep e a cada 10 minutos.

## Conteúdo

- [Dispositivos suportados](#dispositivos-suportados)
- [Instalação](#instalação)
- [Uso](#uso)
- [Como funciona](#como-funciona)
- [Solução de problemas](#solução-de-problemas)
- [Linha de comando e configuração](#linha-de-comando-e-configuração)
- [Compilar](#compilar)
- [Releases e pipeline](#releases-e-pipeline)
- [Limitações](#limitações)
- [Desinstalar](#desinstalar)
- [Contribuir](#contribuir)

## Dispositivos suportados

| Conexão | Como a bateria é lida | Estado |
|---|---|---|
| **Bluetooth** (fones, teclados, mouses) | Propriedade de bateria que o Windows já mantém (a mesma de Configurações › Bluetooth), no nó do dispositivo ou no nó Hands-Free | testado: fone AL-652 (80%) e teclado GMK67 por Bluetooth (34%) |
| **Celular Android por USB** | `adb shell dumpsys battery`, com depuração USB ativa. O servidor ADB é iniciado só durante a leitura e encerrado depois | testado: Samsung SM-A315G |
| **Mouse BK-R3X** (receptor 2.4 GHz, 249A:5C2F) | Relatório proprietário que o mouse envia quando a bateria muda ou ao ligar/desligar. Só escuta; o último valor fica salvo em disco | testado |
| **USB HID com bateria padrão** (Battery Strength, 0x06/0x20) | Lido do descritor HID | implementado, sem hardware para testar |
| **Outros receptores 2.4 GHz** (por exemplo o do teclado GMK67) | Não informam bateria; aparecem com `?`. Use *Sondar dispositivos USB* para investigar | sem suporte |
| **Bluetooth LE via GATT (0x180F)** | Só pela propriedade do Windows, se ela existir | não validado |

O `adb.exe` é procurado no `PATH`, em `%LOCALAPPDATA%\Android\Sdk\platform-tools` e na cópia do Iriun Webcam.

> Para o teclado **GMK67**, o percentual só aparece quando ele está pareado por **Bluetooth**. Por cabo ou por 2.4 GHz ele não expõe a bateria (o software do fabricante também não mostra).

## Instalação

**Opção 1: script (recomendado).** Baixe o `.zip` da [última release](../../releases/latest), extraia e rode no PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

Instala em `%LOCALAPPDATA%\Programs\taskbar-battery\` (sem administrador), cria o atalho "Taskbar Battery" no Menu Iniciar, liga *Iniciar com o Windows* e registra o programa em Configurações › Aplicativos. Opções: `-NoStartup`, `-NoLaunch`, `-InstallDir <pasta>`, `-DryRun`.

**Opção 2: manual.** Baixe o `taskbar-battery-vX.Y.Z-windows-x64.exe`, coloque numa pasta fixa (não rode da pasta Downloads) e execute. *Iniciar com o Windows* usa o caminho de onde o exe estiver.

> **SmartScreen:** o executável não é assinado digitalmente, então o Windows pode avisar na primeira execução (*Mais informações › Executar assim mesmo*). Confira o hash em `SHA256SUMS.txt`, ou [compile você mesmo](#compilar).

**Requisitos:** Windows 11 (outras versões não testadas). ADB só é necessário para o celular.

## Uso

Os ícones novos podem aparecer primeiro no overflow ("^"). O app grava a preferência **"Fixar ícones na barra"** do Windows (ligada por padrão), mas o Windows 11 só relê esse valor quando o Explorer reinicia: na primeira vez, arraste os ícones para a barra ou faça logoff/login.

**Clique direito em qualquer ícone:**

| Item | O que faz |
|---|---|
| Lista de dispositivos | Cada dispositivo com caixa de seleção: desmarque para ocultar, marque para voltar (inclusive os desconectados que você ocultou) |
| Atualizar agora | Lê tudo imediatamente |
| Estilo do ícone | Alterna bateria → número → tipo + barra |
| Alerta de bateria baixa (15%) | Aviso do Windows uma vez por descarga; rearma ao carregar ou passar de 20%. Desligado por padrão |
| Fixar ícones na barra | Marca os ícones como "mostrar na barra" no Windows 11 |
| Iniciar com o Windows | Chave `HKCU\...\Run` |
| Sondar dispositivos USB (60 s) | Escuta o que as interfaces HID USB enviam e grava `%LOCALAPPDATA%\taskbar-battery\probe.txt`. Não digite durante o teste |
| Copiar diagnóstico | Copia as últimas linhas do log para colar num relato |
| Sair | Encerra e remove os ícones |

**O que o ícone mostra**

| Ícone | Significado |
|---|---|
| Nível de bateria/número/barra | Valor atual |
| Vermelho | Abaixo de 20% |
| `?` | Dispositivo detectado, mas sem bateria legível |
| Esmaecido | Último valor conhecido (a leitura atual falhou; expira após ~30 min). No mouse BK-R3X, valor com mais de 12 h |

Passe o mouse sobre o ícone para ver o nome e o status completos.

## Como funciona

- **Janela oculta + `GetMessageW`:** o processo não faz polling em laço. Ele só acorda por timer (10 min), `WM_DEVICECHANGE` (com debounce de 1,5 s), retorno do sleep (`WM_POWERBROADCAST`) ou clique.
- **Bluetooth:** lê as propriedades PnP (`DEVPKEY_Bluetooth_Battery` `{104EA319-6EE2-4701-BD47-8DDBF425BBE5},2`, conexão e Class of Device) via CfgMgr32, sem WinRT. Quando a bateria não está no nó do dispositivo, procura nos nós de serviço do mesmo endereço (Hands-Free).
- **HID:** enumera interfaces HID USB por dispositivo físico (agrupadas pelo pai USB), procura o uso *Battery Strength* e lê o relatório escalado pela faixa lógica.
- **Celular:** executa `adb shell dumpsys battery` numa thread curta, no máximo uma vez a cada 30 s, e encerra o servidor ADB se foi o app que o iniciou.
- **BK-R3X:** uma thread bloqueada em `ReadFile` na interface proprietária do receptor decodifica `00 C0 <flag> <bateria>`.
- **Ícones:** glifos da fonte *Segoe Fluent Icons* desenhados em bitmap 32 bpp com alfa (um HICON por dispositivo, em cache). Após cada leitura o working set é devolvido ao Windows (`EmptyWorkingSet`).
- **Fixar na barra:** grava `IsPromoted=1` em `HKCU\Control Panel\NotifyIconSettings\<id>` (entrada do Windows para cada ícone).

**Consumo (modo real, medido):** ~0,1 a 0,6 MB de working set, ~2,9 MB de memória privada, 2 a 6 threads, 0% de CPU em repouso. O executável tem ~400 KB.

## Solução de problemas

**O ícone não aparece.** Só aparece quando há ao menos um dispositivo com leitura ou conectado. Veja o overflow ("^") e o Gerenciador de Tarefas. A instância é única: se já está rodando, abrir de novo não faz nada.

**Aparece só `?`.** O dispositivo não informa bateria por esse caminho (veja a tabela). Para Bluetooth, confira se o Windows mostra a bateria em Configurações › Bluetooth e dispositivos.

**O celular não aparece.** Precisa de cabo, depuração USB ativa e o `adb.exe` acessível. Autorize o computador na tela do celular. Sem o `adb.exe` o ícone mostra "ADB não encontrado".

**O mouse BK-R3X mostra `?` ou valor antigo.** O mouse só envia o relatório quando a bateria muda ou ao ligar/desligar. Desligue e ligue a chave uma vez.

**Os ícones continuam no overflow.** Faça logoff/login (ou reinicie o Explorer) para o Windows aplicar a preferência. Para desligar, desmarque *Fixar ícones na barra*.

**Mande um relato.** Clique direito › *Copiar diagnóstico* e cole no relato. O log (`%LOCALAPPDATA%\taskbar-battery\diagnostics.log`, até ~512 KB) tem nomes e percentuais dos dispositivos; não tem endereços Bluetooth, seriais nem caminhos de arquivo.

### Sondar um dispositivo não suportado

*Sondar dispositivos USB (60 s)* só **escuta**: nunca envia comandos nem escreve nos dispositivos. O arquivo contém VID/PID, nomes de produto, páginas de uso, tamanhos de relatório e amostras dos relatórios recebidos. Não contém seriais, caminhos nem nomes de usuário. Interfaces proprietárias de teclados podem carregar teclas digitadas: **não digite durante o teste**. Nada é enviado para fora; confira o arquivo antes de colar em qualquer lugar.

## Linha de comando e configuração

```text
taskbar-battery.exe --once            # imprime os dispositivos detectados e sai
taskbar-battery.exe --once --fake 15  # idem, com dados de teste
taskbar-battery.exe --fake 15         # ícones de teste, sem hardware
taskbar-battery.exe --probe 60        # sondagem USB (grava probe.txt)
taskbar-battery.exe --version
```

`%APPDATA%\taskbar-battery\config.ini`:

```ini
hidden=AABBCCDD:Nome do dispositivo   # ocultos (id em hex + nome para o menu)
style=0                               # 0 bateria, 1 número, 2 tipo + barra
alerts=0                              # alerta de bateria baixa
pin=1                                 # fixar ícones na barra
```

Outros arquivos: `%LOCALAPPDATA%\taskbar-battery\` (`diagnostics.log`, `probe.txt`, `last-bkr3x.txt`) e a chave `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\TaskbarBattery`.

## Compilar

Requer Rust estável (1.88 ou mais novo) e as Build Tools do Visual Studio (MSVC).

```powershell
cargo build --release     # target\release\taskbar-battery.exe
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

O C runtime é ligado estaticamente (`.cargo/config.toml`), então o exe não depende de `vcruntime140.dll`.

Estrutura:

```text
src/
├── main.rs      janela oculta, ícones, menu, ciclo de leitura
├── bt.rs        Bluetooth (propriedades PnP, tipo do dispositivo)
├── hid.rs       dispositivos USB HID (agrupados por pai USB)
├── vendor.rs    receptor do mouse BK-R3X (relatório proprietário)
├── phone.rs     celular Android via ADB
├── icon.rs      desenho dos ícones (3 estilos)
├── pin.rs       "fixar na barra" (IsPromoted)
├── config.rs    config.ini        diag.rs   log de diagnóstico
├── startup.rs   Iniciar com o Windows
└── probe.rs     sondagem de interfaces HID (somente escuta)
scripts/         install.ps1, uninstall.ps1
tools/probe/     sonda de desenvolvimento (HID + WPD), crate separado
```

## Releases e pipeline

- **CI** (`.github/workflows/ci.yml`): em cada push na `main` e em pull requests, no `windows-latest`: `cargo fmt --check`, `clippy -D warnings`, `cargo test`, build release, *smoke test* sem hardware (`--version` e `--once --fake 42`) e upload do `.exe` como artefato.
- **Release** (`.github/workflows/release.yml`): ao enviar uma tag `vX.Y.Z`, valida que a tag bate com a versão do `Cargo.toml`, roda as verificações, compila e publica uma release no GitHub com:
  - `taskbar-battery-vX.Y.Z-windows-x64.zip` (exe, README, LICENSE, CHANGELOG, `install.ps1`, `uninstall.ps1`);
  - `taskbar-battery-vX.Y.Z-windows-x64.exe`;
  - `SHA256SUMS.txt`.

  As notas da release saem da seção correspondente do `CHANGELOG.md`. Tags com sufixo (`v0.2.0-rc.1`) saem como pré-release.
- **Dependabot** atualiza dependências do Cargo e das Actions toda semana.

Para lançar uma versão:

```powershell
# 1. atualize a versão em Cargo.toml e adicione a seção "## [X.Y.Z] - AAAA-MM-DD" ao CHANGELOG.md
cargo build --release     # atualiza o Cargo.lock
git commit -am "chore: release vX.Y.Z"
git tag vX.Y.Z
git push origin main vX.Y.Z
```

## Limitações

- Só a barra principal; Windows 11 (outras versões não testadas).
- Receptores 2.4 GHz proprietários dependem de engenharia reversa de cada protocolo, e muitos só respondem a comandos (o app nunca envia comandos a dispositivos).
- Sem carregamento para Bluetooth e para o mouse BK-R3X (o canal não informa).
- Bluetooth LE via GATT (WinRT) não está implementado.
- O executável não tem ícone/versão embutidos nem assinatura digital.

## Desinstalar

Configurações › Aplicativos › *Taskbar Battery*, ou:

```powershell
powershell -ExecutionPolicy Bypass -File "$env:LOCALAPPDATA\Programs\taskbar-battery\uninstall.ps1"
```

Ele encerra o app e remove a inicialização, o atalho, o registro, as configurações e a pasta. Use `-KeepSettings` para manter `config.ini` e os arquivos em `%LOCALAPPDATA%\taskbar-battery`.

## Contribuir

Relatos de dispositivos (o que funciona e o que não) e PRs são bem-vindos. Antes de abrir um PR rode `cargo fmt`, `cargo clippy --all-targets -- -D warnings` e `cargo test`. Para um dispositivo novo, adicione um módulo ao lado de `bt.rs`/`hid.rs` que devolva uma lista de `Device`.

Mensagens de commit seguem [Conventional Commits](https://www.conventionalcommits.org/pt-br/).

## Licença

[MIT](LICENSE)
