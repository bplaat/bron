/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! v86setup.exe - one shot setup of the fresh install on its first v86 boot, run at the
//! Administrator logon from the Startup folder (see winxp-image/disk.mjs). Expects its files next
//! to it in `%SystemRoot%\v86`, installs the guest integration, tunes Windows for v86, activates
//! it offline, creates the bplaat account and reboots into it. The software is already installed
//! by winxp-image/software.mjs.

#![windows_subsystem = "windows"]

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::{env, fs, ptr};

use v86::win32::*;

const USER_NAME: &str = "bplaat";
const USER_FULL_NAME: &str = "Bastiaan";
const DEFAULT_HIVE: &str = "V86Default";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const MEDIA_CLASS: GUID = GUID(
    0x4d36e96c,
    0xe325,
    0x11ce,
    [0xbf, 0xc1, 0x08, 0x00, 0x2b, 0xe1, 0x03, 0x18],
);

enum Data {
    Sz(&'static str),
    Dword(u32),
}
use Data::{Dword, Sz};

/// A registry value: key, name (None for the default value) and data
type Value = (&'static str, Option<&'static str>, Data);

macro_rules! key {
    (CV $path:literal) => {
        concat!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\", $path)
    };
    (CCS $path:literal) => {
        concat!(r"SYSTEM\CurrentControlSet\", $path)
    };
    (EXPLORER $path:literal) => {
        concat!(
            r"Software\Microsoft\Windows\CurrentVersion\Explorer\",
            $path
        )
    };
    (WMP) => {
        r"Software\Microsoft\MediaPlayer\Preferences"
    };
}

const SECURITY_CENTER: &str = r"SOFTWARE\Microsoft\Security Center";
const WINLOGON: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon";
const DESKTOP: &str = r"Control Panel\Desktop";
const ADVANCED: &str = key!(EXPLORER "Advanced");
const AGENT_COMMAND: &str = r#""%SystemRoot%\system32\v86agent.exe" /open "%1""#;

/// Machine settings: nothing that nags, nothing that runs in the background without need
const MACHINE: &[Value] = &[
    (SECURITY_CENTER, Some("AntiVirusDisableNotify"), Dword(1)),
    (SECURITY_CENTER, Some("FirewallDisableNotify"), Dword(1)),
    (SECURITY_CENTER, Some("UpdatesDisableNotify"), Dword(1)),
    (SECURITY_CENTER, Some("FirstRunDisabled"), Dword(1)),
    (key!(CV "Applets\\Tour"), Some("RunCount"), Dword(0)),
    (
        r"SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate\AU",
        Some("NoAutoUpdate"),
        Dword(1),
    ),
    (
        r"SOFTWARE\Policies\Microsoft\Messenger\Client",
        Some("PreventRun"),
        Dword(1),
    ),
    (
        r"SOFTWARE\Policies\Microsoft\WindowsMediaPlayer",
        Some("GroupPrivacyAcceptance"),
        Dword(1),
    ),
    (
        r"SOFTWARE\Policies\Microsoft\WindowsMediaPlayer",
        Some("DisableAutoUpdate"),
        Dword(1),
    ),
    (
        r"SOFTWARE\Microsoft\PCHealth\ErrorReporting",
        Some("DoReport"),
        Dword(0),
    ),
    (
        r"SOFTWARE\Microsoft\PCHealth\ErrorReporting",
        Some("ShowUI"),
        Dword(0),
    ),
    (
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\SystemRestore",
        Some("DisableSR"),
        Dword(1),
    ),
    (
        r"SOFTWARE\Microsoft\Dfrg\BootOptimizeFunction",
        Some("Enable"),
        Sz("N"),
    ),
    (key!(CV "OptimalLayout"), Some("EnableAutoLayout"), Dword(0)),
    (
        key!(CCS "Control\\Session Manager\\Memory Management\\PrefetchParameters"),
        Some("EnablePrefetcher"),
        Dword(0),
    ),
    (
        key!(CCS "Control\\CrashControl"),
        Some("CrashDumpEnabled"),
        Dword(0),
    ),
    (
        key!(CCS "Control\\CrashControl"),
        Some("SendAlert"),
        Dword(0),
    ),
    (
        key!(CCS "Control\\Terminal Server"),
        Some("fAllowToGetHelp"),
        Dword(0),
    ),
    // Inserted install CDs must not start their own setup
    (
        key!(CV "Policies\\Explorer"),
        Some("NoDriveTypeAutoRun"),
        Dword(0xFF),
    ),
    (
        key!(CCS "Services\\SharedAccess\\Parameters\\FirewallPolicy\\StandardProfile"),
        Some("EnableFirewall"),
        Dword(0),
    ),
    // Logs on as bplaat without a password
    (WINLOGON, Some("DefaultUserName"), Sz(USER_NAME)),
    (WINLOGON, Some("DefaultDomainName"), Sz("WINXP")),
    (WINLOGON, Some("DefaultPassword"), Sz("")),
    (WINLOGON, Some("AutoAdminLogon"), Sz("1")),
    (
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts",
        Some("MS Gothic & MS PGothic & MS UI Gothic (TrueType)"),
        Sz("msgothic.ttc"),
    ),
    // Links open in a browser tab on the host, the Start menu still shows Internet Explorer as
    // Internet
    (
        r"SOFTWARE\Clients\StartMenuInternet",
        None,
        Sz("IEXPLORE.EXE"),
    ),
    (
        r"SOFTWARE\Classes\http\shell\open\command",
        None,
        Sz(AGENT_COMMAND),
    ),
    (
        r"SOFTWARE\Classes\https\shell\open\command",
        None,
        Sz(AGENT_COMMAND),
    ),
];

/// Services v86 has no use for: the LAN only needs the DHCP client, no printer, no updates. System
/// Information needs Help and Support, so that one keeps its default startup setting.
const SERVICES: &[&str] = &[
    "Alerter",
    "Browser",
    "Dnscache",
    "ERSvc",
    "lanmanserver",
    "LmHosts",
    "Messenger",
    "Nla",
    "RasMan",
    "RemoteRegistry",
    "Schedule",
    "seclogon",
    "SharedAccess",
    "ShellHWDetection",
    "Spooler",
    "srservice",
    "SSDPSRV",
    "stisvc",
    "TapiSrv",
    "TrkWks",
    "upnphost",
    "W32Time",
    "WebClient",
    "wscsvc",
    "wuauserv",
    "WZCSVC",
];

/// User settings, written to the Default User profile so the new account starts with them
const USER: &[Value] = &[
    (
        DESKTOP,
        Some("Wallpaper"),
        Sz(r"C:\WINDOWS\Web\Wallpaper\Bliss.bmp"),
    ),
    (DESKTOP, Some("WallpaperStyle"), Sz("2")),
    (DESKTOP, Some("TileWallpaper"), Sz("0")),
    (DESKTOP, Some("ScreenSaveActive"), Sz("0")),
    (
        r"Control Panel\Desktop\WindowMetrics",
        Some("MinAnimate"),
        Sz("0"),
    ),
    (
        key!(EXPLORER "VisualEffects"),
        Some("VisualFXSetting"),
        Dword(3),
    ),
    (ADVANCED, Some("EnableBalloonTips"), Dword(0)),
    (ADVANCED, Some("StartButtonBalloonTip"), Dword(0)),
    (ADVANCED, Some("Start_NotifyNewApps"), Dword(0)),
    (ADVANCED, Some("TaskbarAnimations"), Dword(0)),
    (ADVANCED, Some("ListviewAlphaSelect"), Dword(0)),
    // Folder Options: hidden and system files, extensions, full paths, Details view in every folder
    (ADVANCED, Some("Hidden"), Dword(1)),
    (ADVANCED, Some("ShowSuperHidden"), Dword(1)),
    (ADVANCED, Some("HideFileExt"), Dword(0)),
    (ADVANCED, Some("FullPathAddress"), Dword(1)),
    (ADVANCED, Some("WebViewBarricade"), Dword(1)),
    (key!(EXPLORER "CabinetState"), Some("FullPath"), Dword(1)),
    (
        r"Software\Microsoft\Windows\ShellNoRoam\Bags\AllFolders\Shell",
        Some("Mode"),
        Dword(4),
    ),
    (
        r"Software\Microsoft\Windows\ShellNoRoam\Bags\AllFolders\Shell",
        Some("Vid"),
        Sz("{137E7700-3573-11CF-AE69-08002B2E1262}"),
    ),
    (
        key!(EXPLORER "HideDesktopIcons\\NewStartPanel"),
        Some("{20D04FE0-3AEA-1069-A2D8-08002B30309D}"),
        Dword(0),
    ),
    (
        key!(EXPLORER "HideDesktopIcons\\NewStartPanel"),
        Some("{450D8FBA-AD25-11D0-98A8-0800361B1103}"),
        Dword(0),
    ),
    (
        key!(EXPLORER "Desktop\\CleanupWiz"),
        Some("NoRun"),
        Dword(1),
    ),
    (
        r"Software\Microsoft\Windows\CurrentVersion\Applets\Tour",
        Some("RunCount"),
        Dword(0),
    ),
    (
        r"Software\Microsoft\Windows\CurrentVersion\Policies\Explorer",
        Some("NoDriveTypeAutoRun"),
        Dword(0xFF),
    ),
    (
        r"Software\Microsoft\Windows\CurrentVersion\Policies\Explorer",
        Some("NoLowDiskSpaceChecks"),
        Dword(1),
    ),
    (
        r"Software\Microsoft\Internet Explorer\Main",
        Some("Check_Associations"),
        Sz("no"),
    ),
    (
        r"Software\Clients\StartMenuInternet",
        None,
        Sz("IEXPLORE.EXE"),
    ),
    // Media Player 9: no first run wizard or internet lookups, menu bar shown, Now Playing shows
    // the album art
    (key!(WMP), Some("AcceptedPrivacyStatement"), Dword(1)),
    (key!(WMP), Some("MetadataRetrieval"), Dword(0)),
    (key!(WMP), Some("SilentAcquisition"), Dword(0)),
    (key!(WMP), Some("UsageTracking"), Dword(0)),
    (key!(WMP), Some("FirstRun"), Dword(0)),
    (key!(WMP), Some("ShowAppTitlebar"), Dword(1)),
    (key!(WMP), Some("ShowEffects"), Dword(1)),
    (key!(WMP), Some("ShowAlbumArt"), Dword(1)),
    (key!(WMP), Some("CurrentDisplayView"), Sz("AlbumArtView")),
    (key!(WMP), Some("UserDisplayView"), Sz("AlbumArtView")),
    (key!(WMP), Some("UserWMPDisplayView"), Sz("AlbumArtView")),
    // No "Display Settings" offer to fix a small screen, no system sounds
    (
        key!(EXPLORER "DontShowMeThisDialogAgain"),
        Some("ScreenCheck"),
        Sz("no"),
    ),
    (r"AppEvents\Schemes", None, Sz(".None")),
];

/// No menu, tooltip or selection animations; keeps hot tracking, gradient captions and flat
/// menus, without them Luna paints menu bar items in the white menu colour on the beige bar
const PREFERENCES_MASK: [u8; 4] = [0x90, 0x12, 0x03, 0x80];

fn set_value(root: &Key, key: &str, name: Option<&str>, kind: u32, data: &[u8]) {
    if let Some(key) = root.create(key, KEY_SET_VALUE) {
        key.set(name, kind, data);
    }
}

fn set_string(root: &Key, key: &str, name: Option<&str>, value: &str) {
    // Values with environment variables expand when read
    let kind = if value.contains('%') {
        REG_EXPAND_SZ
    } else {
        REG_SZ
    };
    set_value(root, key, name, kind, &utf16_bytes(&wide(value)));
}

fn set_multi(root: &Key, key: &str, name: &str, values: &[&str]) {
    set_value(
        root,
        key,
        Some(name),
        REG_MULTI_SZ,
        &utf16_bytes(&wide_multi(values)),
    );
}

fn set_values(root: &Key, values: &[Value]) {
    for (key, name, data) in values {
        match data {
            Sz(value) => set_string(root, key, *name, value),
            Dword(value) => set_value(root, key, *name, REG_DWORD, &value.to_le_bytes()),
        }
    }
}

/// Runs a program hidden and waits for it. Without standard handles: the ones of setup come from
/// the console of the Startup script, which is gone by now, so passing them on fails.
fn run(program: &str, args: &[&str], dir: &Path) {
    _ = Command::new(program)
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

/// Copies a file of the setup to `system32\<subdir>`, returns its new path
fn copy_to_system(dir: &Path, name: &str, subdir: &str) -> PathBuf {
    let target = Path::new(&expand(r"%SystemRoot%\system32"))
        .join(subdir)
        .join(name);
    _ = fs::copy(dir.join(name), &target);
    target
}

fn update_driver(hardware_id: &str, inf: &str) {
    type UpdateDriver =
        unsafe extern "system" fn(HWND, *const u16, *const u16, u32, *mut BOOL) -> BOOL;
    const INSTALLFLAG_FORCE: u32 = 1;
    let (library, hardware_id, inf) = (w!("newdev.dll"), wide(hardware_id), wide(inf));
    // SAFETY: The strings are NUL terminated and the export has this signature
    unsafe {
        let newdev = LoadLibraryW(library.as_ptr());
        if newdev.is_null() {
            return;
        }
        let update = GetProcAddress(
            newdev,
            c"UpdateDriverForPlugAndPlayDevicesW".as_ptr().cast(),
        );
        if !update.is_null() {
            let update = std::mem::transmute::<*mut core::ffi::c_void, UpdateDriver>(update);
            let mut reboot = 0;
            update(
                ptr::null_mut(),
                hardware_id.as_ptr(),
                inf.as_ptr(),
                INSTALLFLAG_FORCE,
                &mut reboot,
            );
        }
    }
}

/// Sound: the Sound Blaster 16 isn't Plug and Play, so it's added as a root device like Add
/// Hardware does; the first resource choices of the INF (220h, IRQ 5, DMA 1 and 5) are what v86
/// emulates
fn install_sound() {
    let hardware_id = utf16_bytes(&wide_multi(&["*PNPB003"]));
    let class_name = w!("MEDIA");
    // SAFETY: The class, the name and the hardware ID are valid for the calls, the set is
    // destroyed after
    unsafe {
        let set = SetupDiCreateDeviceInfoList(&MEDIA_CLASS, ptr::null_mut());
        if set as isize == -1 {
            return;
        }
        let mut device = SP_DEVINFO_DATA {
            cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
            ClassGuid: [0; 16],
            DevInst: 0,
            Reserved: 0,
        };
        if SetupDiCreateDeviceInfoW(
            set,
            class_name.as_ptr(),
            &MEDIA_CLASS,
            ptr::null(),
            ptr::null_mut(),
            DICD_GENERATE_ID,
            &mut device,
        ) != 0
            && SetupDiSetDeviceRegistryPropertyW(
                set,
                &mut device,
                SPDRP_HARDWAREID,
                hardware_id.as_ptr(),
                hardware_id.len() as u32,
            ) != 0
            && SetupDiCallClassInstaller(DIF_REGISTERDEVICE, set, &mut device) != 0
        {
            update_driver("*PNPB003", &expand(r"%SystemRoot%\inf\wdma_ctl.inf"));
        }
        SetupDiDestroyDeviceInfoList(set);
    }
}

/// System start drivers: the host channel loads independently of the mouse filter
fn install_kernel_driver(dir: &Path, name: &str, display_name: &str, group: Option<&str>) {
    let target = copy_to_system(dir, &format!("{name}.sys"), "drivers");
    let (name, display_name, target) = (
        wide(name),
        wide(display_name),
        wide(target.to_string_lossy().as_ref()),
    );
    let group = group.map(wide);
    // SAFETY: The strings are NUL terminated or null, the handles are closed after
    unsafe {
        let manager = OpenSCManagerW(ptr::null(), ptr::null(), SC_MANAGER_CREATE_SERVICE);
        if manager.is_null() {
            return;
        }
        let service = CreateServiceW(
            manager,
            name.as_ptr(),
            display_name.as_ptr(),
            SERVICE_ALL_ACCESS,
            SERVICE_KERNEL_DRIVER,
            SERVICE_SYSTEM_START,
            SERVICE_ERROR_NORMAL,
            target.as_ptr(),
            group.as_ref().map_or(ptr::null(), |group| group.as_ptr()),
            ptr::null_mut(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
        );
        if !service.is_null() {
            CloseServiceHandle(service);
        }
        CloseServiceHandle(manager);
    }
}

/// The agent starts at every logon and stands in as the web browser
fn install_agent(dir: &Path) {
    let target = copy_to_system(dir, "v86agent.exe", "");
    set_string(
        &Key::LOCAL_MACHINE,
        key!(CV "Run"),
        Some("v86agent"),
        &target.to_string_lossy(),
    );
    for scheme in ["http", "https"] {
        for subkey in [r"\Application", r"\Topic", ""] {
            Key::LOCAL_MACHINE.delete(&format!(
                r"SOFTWARE\Classes\{scheme}\shell\open\ddeexec{subkey}"
            ));
        }
    }
}

fn tune_machine(dir: &Path) {
    set_values(&Key::LOCAL_MACHINE, MACHINE);
    for service in SERVICES {
        if let Some(key) = Key::LOCAL_MACHINE.open(
            &format!(r"SYSTEM\CurrentControlSet\Services\{service}"),
            KEY_SET_VALUE,
        ) {
            key.set_dword("Start", SERVICE_DISABLED);
        }
    }
    // Japanese text (like song names) falls back to MS Gothic in the interface fonts
    for font in [
        "Tahoma",
        "MS Shell Dlg",
        "Microsoft Sans Serif",
        "Trebuchet MS",
    ] {
        set_multi(
            &Key::LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\FontLink\SystemLink",
            font,
            &["MSGOTHIC.TTC,MS UI Gothic"],
        );
    }
    // No pagefile and no hibernation file: the disk streams over the network
    set_multi(
        &Key::LOCAL_MACHINE,
        key!(CCS "Control\\Session Manager\\Memory Management"),
        "PagingFiles",
        &[""],
    );
    if let Some(key) = Key::LOCAL_MACHINE.open(WINLOGON, KEY_SET_VALUE) {
        key.delete_value("AutoLogonCount");
    }
    run("powercfg.exe", &["/hibernate", "off"], dir);
}

/// Every event of the sound scheme gets no sound
fn silence_sounds(root: &Key) {
    let Some(apps) = root.open(r"AppEvents\Schemes\Apps", KEY_READ) else {
        return;
    };
    for app in apps.subkeys() {
        let Some(events) = apps.open(&app, KEY_READ) else {
            continue;
        };
        for event in events.subkeys() {
            set_string(
                root,
                &format!(r"AppEvents\Schemes\Apps\{app}\{event}\.Current"),
                None,
                "",
            );
        }
    }
}

fn tune_user(root: &Key) {
    set_values(root, USER);
    set_value(
        root,
        DESKTOP,
        Some("UserPreferencesMask"),
        REG_BINARY,
        &PREFERENCES_MASK,
    );
    silence_sounds(root);
}

/// Settings go into the Default User profile, so the new account starts with them, but the per
/// user setup of Windows (Active Setup) resets some at the first logon, so the account applies
/// them once more after it, see [`user_logon`]
fn tune_default_user(dir: &Path) {
    let Some((hive, profiles)) = Key::LOCAL_MACHINE
        .open(
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList",
            KEY_QUERY_VALUE,
        )
        .and_then(|key| {
            Some((
                key.string("DefaultUserProfile")?,
                key.string("ProfilesDirectory")?,
            ))
        })
    else {
        return;
    };
    let file = format!(r"{}\{hive}\NTUSER.DAT", expand(&profiles));
    if !Key::USERS.load(DEFAULT_HIVE, &file) {
        return;
    }
    if let Some(root) = Key::USERS.open(DEFAULT_HIVE, KEY_ALL_ACCESS) {
        tune_user(&root);
        let command = format!(r#""{}" /user"#, dir.join("v86setup.exe").display());
        set_string(&root, key!(CV "RunOnce"), Some("v86user"), &command);
    }
    Key::USERS.unload(DEFAULT_HIVE);
}

/// The account the desktop logs on with, with its picture on the Start menu
fn create_user(dir: &Path) {
    let (name, password, full_name, group) = (
        w!(USER_NAME),
        w!(""),
        w!(USER_FULL_NAME),
        w!("Administrators"),
    );
    let info = USER_INFO_1 {
        name: name.as_ptr(),
        password: password.as_ptr(),
        password_age: 0,
        privilege: USER_PRIV_USER,
        home_dir: ptr::null(),
        comment: ptr::null(),
        flags: UF_SCRIPT | UF_DONT_EXPIRE_PASSWD,
        script_path: ptr::null(),
    };
    // SAFETY: The user infos point to NUL terminated strings that outlive the calls
    unsafe {
        NetUserAdd(ptr::null(), 1, (&raw const info).cast(), ptr::null_mut());
        // USER_INFO_1011 and LOCALGROUP_MEMBERS_INFO_3 are one string pointer
        let full_name = full_name.as_ptr();
        NetUserSetInfo(
            ptr::null(),
            name.as_ptr(),
            1011,
            (&raw const full_name).cast(),
            ptr::null_mut(),
        );
        let member = name.as_ptr();
        NetLocalGroupAddMembers(
            ptr::null(),
            group.as_ptr(),
            3,
            (&raw const member).cast(),
            1,
        );
    }
    let pictures = expand(r"%ALLUSERSPROFILE%\Application Data\Microsoft\User Account Pictures");
    _ = fs::copy(
        dir.join("user.bmp"),
        Path::new(&pictures).join(format!("{USER_NAME}.bmp")),
    );
}

/// First logon of the account, after Active Setup: the user settings again and the Media Player
/// library, which resets some Media Player settings, so those once more after it
fn user_logon(dir: &Path) {
    tune_user(&Key::CURRENT_USER);
    run(
        "wscript.exe",
        &["//B", &dir.join("library.vbs").to_string_lossy()],
        dir,
    );
    tune_user(&Key::CURRENT_USER);
    let environment = w!("Environment");
    // SAFETY: The string is NUL terminated
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            environment.as_ptr() as LPARAM,
            SMTO_ABORTIFHUNG,
            5000,
            ptr::null_mut(),
        )
    };
}

fn main() {
    let Some(dir) = env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    else {
        return;
    };
    if env::args()
        .nth(1)
        .is_some_and(|arg| arg.eq_ignore_ascii_case("/user"))
    {
        user_logon(&dir);
        return;
    }
    enable_privilege("SeBackupPrivilege");
    enable_privilege("SeRestorePrivilege");
    enable_privilege("SeShutdownPrivilege");

    install_kernel_driver(&dir, "v86tools", "v86 Host Channel", None);
    // The mouse is a system start filter above mouclass
    install_kernel_driver(&dir, "v86mou", "v86 Mouse", Some("Pointer Class"));
    set_multi(
        &Key::LOCAL_MACHINE,
        key!(CCS "Control\\Class\\{4D36E96F-E325-11CE-BFC1-08002BE10318}"),
        "UpperFilters",
        &["mouclass", "v86mou"],
    );
    install_agent(&dir);
    // Display: binds the Bochs VBE adapter to v86vid through its INF
    update_driver(
        r"PCI\VEN_1234&DEV_1111",
        &dir.join("v86vid.inf").to_string_lossy(),
    );
    install_sound();
    // Network: the NE2000 of v86 is a Realtek RTL8029, Windows XP has the driver; the site
    // connects it to the LAN relay
    update_driver(
        r"PCI\VEN_10EC&DEV_8029",
        &expand(r"%SystemRoot%\inf\netrtpnt.inf"),
    );
    tune_machine(&dir);
    tune_default_user(&dir);
    create_user(&dir);
    // Offline activation: activate.vbs feeds the installation ID through confid.exe, it reports
    // over COM1
    run(
        "cscript.exe",
        &["//nologo", &dir.join("activate.vbs").to_string_lossy()],
        &dir,
    );
    // SAFETY: Plain call, the shutdown privilege is enabled
    unsafe { ExitWindowsEx(EWX_REBOOT | EWX_FORCE, 0) };
}
