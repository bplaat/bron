/*
 * v86setup.exe - one shot setup of the fresh install on its first v86 boot, run at the Administrator logon from the
 * Startup folder (see winxp-image/disk.mjs). Expects its files next to it in %SystemRoot%\v86, installs the guest
 * integration, tunes Windows for v86, activates it offline, creates the bplaat account and reboots into it. The
 * software is already installed by winxp-image/software.mjs.
 */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <lm.h>
#include <setupapi.h>

#define USER_NAME L"bplaat"
#define USER_FULL_NAME L"Bastiaan"
#define DEFAULT_HIVE L"V86Default"

typedef BOOL(WINAPI *UPDATE_DRIVER)(HWND, LPCWSTR, LPCWSTR, DWORD, PBOOL);
static const GUID media_class = {0x4d36e96c, 0xe325, 0x11ce, {0xbf, 0xc1, 0x08, 0x00, 0x2b, 0xe1, 0x03, 0x18}};

typedef struct {
    const WCHAR *key, *name;
    DWORD type;
    const WCHAR *sz;
    DWORD dw;
} REG_VALUE;
#define SZ(key, name, value) {key, name, REG_SZ, value, 0}
#define DW(key, name, value) {key, name, REG_DWORD, NULL, value}

#define CV L"SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\"
#define CCS L"SYSTEM\\CurrentControlSet\\"

/* Machine settings: nothing that nags, nothing that runs in the background without need */
static const REG_VALUE machine[] = {
    DW(L"SOFTWARE\\Microsoft\\Security Center", L"AntiVirusDisableNotify", 1),
    DW(L"SOFTWARE\\Microsoft\\Security Center", L"FirewallDisableNotify", 1),
    DW(L"SOFTWARE\\Microsoft\\Security Center", L"UpdatesDisableNotify", 1),
    DW(L"SOFTWARE\\Microsoft\\Security Center", L"FirstRunDisabled", 1),
    DW(CV L"Applets\\Tour", L"RunCount", 0),
    DW(L"SOFTWARE\\Policies\\Microsoft\\Windows\\WindowsUpdate\\AU", L"NoAutoUpdate", 1),
    DW(L"SOFTWARE\\Policies\\Microsoft\\Messenger\\Client", L"PreventRun", 1),
    DW(L"SOFTWARE\\Policies\\Microsoft\\WindowsMediaPlayer", L"GroupPrivacyAcceptance", 1),
    DW(L"SOFTWARE\\Policies\\Microsoft\\WindowsMediaPlayer", L"DisableAutoUpdate", 1),
    DW(L"SOFTWARE\\Microsoft\\PCHealth\\ErrorReporting", L"DoReport", 0),
    DW(L"SOFTWARE\\Microsoft\\PCHealth\\ErrorReporting", L"ShowUI", 0),
    DW(L"SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\SystemRestore", L"DisableSR", 1),
    SZ(L"SOFTWARE\\Microsoft\\Dfrg\\BootOptimizeFunction", L"Enable", L"N"),
    DW(CV L"OptimalLayout", L"EnableAutoLayout", 0),
    DW(CCS L"Control\\Session Manager\\Memory Management\\PrefetchParameters", L"EnablePrefetcher", 0),
    DW(CCS L"Control\\CrashControl", L"CrashDumpEnabled", 0),
    DW(CCS L"Control\\CrashControl", L"SendAlert", 0),
    DW(CCS L"Control\\Terminal Server", L"fAllowToGetHelp", 0),
    /* Inserted install CDs must not start their own setup */
    DW(CV L"Policies\\Explorer", L"NoDriveTypeAutoRun", 0xFF),
    DW(CCS L"Services\\SharedAccess\\Parameters\\FirewallPolicy\\StandardProfile", L"EnableFirewall", 0),
    /* Log on as bplaat without a password */
    SZ(L"SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Winlogon", L"DefaultUserName", USER_NAME),
    SZ(L"SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Winlogon", L"DefaultDomainName", L"WINXP"),
    SZ(L"SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Winlogon", L"DefaultPassword", L""),
    SZ(L"SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Winlogon", L"AutoAdminLogon", L"1"),
    SZ(L"SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Fonts", L"MS Gothic & MS PGothic & MS UI Gothic (TrueType)",
       L"msgothic.ttc"),
    /* Links open in a browser tab on the host, the Start menu still shows Internet Explorer as Internet */
    SZ(L"SOFTWARE\\Clients\\StartMenuInternet", NULL, L"IEXPLORE.EXE"),
    SZ(L"SOFTWARE\\Classes\\http\\shell\\open\\command", NULL, L"\"%SystemRoot%\\system32\\v86agent.exe\" /open \"%1\""),
    SZ(L"SOFTWARE\\Classes\\https\\shell\\open\\command", NULL,
       L"\"%SystemRoot%\\system32\\v86agent.exe\" /open \"%1\""),
};

/* Services v86 has no use for: the LAN only needs the DHCP client, no printer, no updates */
static const WCHAR *const services[] = {
    L"Alerter", L"Browser", L"Dnscache", L"ERSvc", L"helpsvc", L"lanmanserver", L"LmHosts",
    L"Messenger", L"Nla", L"RasMan", L"RemoteRegistry", L"Schedule", L"seclogon", L"SharedAccess",
    L"ShellHWDetection", L"Spooler", L"srservice", L"SSDPSRV", L"stisvc", L"TapiSrv", L"TrkWks", L"upnphost",
    L"W32Time", L"WebClient", L"wscsvc", L"wuauserv", L"WZCSVC",
};

/* User settings, written to the Default User profile so the new account starts with them */
#define DESKTOP L"Control Panel\\Desktop"
#define EXPLORER L"Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\"
#define WMP L"Software\\Microsoft\\MediaPlayer\\"
static const REG_VALUE user[] = {
    SZ(DESKTOP, L"Wallpaper", L"C:\\WINDOWS\\Web\\Wallpaper\\Bliss.bmp"),
    SZ(DESKTOP, L"WallpaperStyle", L"2"),
    SZ(DESKTOP, L"TileWallpaper", L"0"),
    SZ(DESKTOP, L"ScreenSaveActive", L"0"),
    SZ(DESKTOP L"\\WindowMetrics", L"MinAnimate", L"0"),
    DW(EXPLORER L"VisualEffects", L"VisualFXSetting", 3),
    DW(EXPLORER L"Advanced", L"EnableBalloonTips", 0),
    DW(EXPLORER L"Advanced", L"StartButtonBalloonTip", 0),
    DW(EXPLORER L"Advanced", L"Start_NotifyNewApps", 0),
    DW(EXPLORER L"Advanced", L"TaskbarAnimations", 0),
    DW(EXPLORER L"Advanced", L"ListviewAlphaSelect", 0),
    /* Folder Options: hidden and system files, extensions, full paths, Details view in every folder */
    DW(EXPLORER L"Advanced", L"Hidden", 1),
    DW(EXPLORER L"Advanced", L"ShowSuperHidden", 1),
    DW(EXPLORER L"Advanced", L"HideFileExt", 0),
    DW(EXPLORER L"Advanced", L"FullPathAddress", 1),
    DW(EXPLORER L"Advanced", L"WebViewBarricade", 1),
    DW(EXPLORER L"CabinetState", L"FullPath", 1),
    DW(L"Software\\Microsoft\\Windows\\ShellNoRoam\\Bags\\AllFolders\\Shell", L"Mode", 4),
    SZ(L"Software\\Microsoft\\Windows\\ShellNoRoam\\Bags\\AllFolders\\Shell", L"Vid",
       L"{137E7700-3573-11CF-AE69-08002B2E1262}"),
    DW(EXPLORER L"HideDesktopIcons\\NewStartPanel", L"{20D04FE0-3AEA-1069-A2D8-08002B30309D}", 0),
    DW(EXPLORER L"HideDesktopIcons\\NewStartPanel", L"{450D8FBA-AD25-11D0-98A8-0800361B1103}", 0),
    DW(EXPLORER L"Desktop\\CleanupWiz", L"NoRun", 1),
    DW(L"Software\\Microsoft\\Windows\\CurrentVersion\\Applets\\Tour", L"RunCount", 0),
    DW(L"Software\\Microsoft\\Windows\\CurrentVersion\\Policies\\Explorer", L"NoDriveTypeAutoRun", 0xFF),
    DW(L"Software\\Microsoft\\Windows\\CurrentVersion\\Policies\\Explorer", L"NoLowDiskSpaceChecks", 1),
    SZ(L"Software\\Microsoft\\Internet Explorer\\Main", L"Check_Associations", L"no"),
    SZ(L"Software\\Clients\\StartMenuInternet", NULL, L"IEXPLORE.EXE"),
    /* Media Player 9: no first run wizard or internet lookups, menu bar shown, Now Playing shows the album art */
    DW(WMP L"Preferences", L"AcceptedPrivacyStatement", 1),
    DW(WMP L"Preferences", L"MetadataRetrieval", 0),
    DW(WMP L"Preferences", L"SilentAcquisition", 0),
    DW(WMP L"Preferences", L"UsageTracking", 0),
    DW(WMP L"Preferences", L"FirstRun", 0),
    DW(WMP L"Preferences", L"ShowAppTitlebar", 1),
    DW(WMP L"Preferences", L"ShowEffects", 1),
    DW(WMP L"Preferences", L"ShowAlbumArt", 1),
    SZ(WMP L"Preferences", L"CurrentDisplayView", L"AlbumArtView"),
    SZ(WMP L"Preferences", L"UserDisplayView", L"AlbumArtView"),
    SZ(WMP L"Preferences", L"UserWMPDisplayView", L"AlbumArtView"),
    /* No "Display Settings" offer to fix a small screen, no system sounds */
    SZ(EXPLORER L"DontShowMeThisDialogAgain", L"ScreenCheck", L"no"),
    SZ(L"AppEvents\\Schemes", NULL, L".None"),
};
/* No menu, tooltip or selection animations; keeps hot tracking, gradient captions and flat menus, without them Luna
   paints menu bar items in the white menu colour on the beige bar */
static const BYTE preferences_mask[] = {0x90, 0x12, 0x03, 0x80};

static WCHAR dir[MAX_PATH];

static void path(WCHAR *out, const WCHAR *name)
{
    wsprintfW(out, L"%s\\%s", dir, name);
}

static void set_value(HKEY root, const WCHAR *key, const WCHAR *name, DWORD type, const void *data, DWORD size)
{
    HKEY h;
    if (RegCreateKeyExW(root, key, 0, NULL, 0, KEY_SET_VALUE, NULL, &h, NULL) != ERROR_SUCCESS) return;
    RegSetValueExW(h, name, 0, type, data, size);
    RegCloseKey(h);
}

static void set_values(HKEY root, const REG_VALUE *values, size_t count)
{
    for (size_t i = 0; i < count; i++) {
        const REG_VALUE *v = &values[i];
        if (v->type == REG_SZ) {
            /* Values with environment variables expand when read */
            DWORD type = wcschr(v->sz, L'%') ? REG_EXPAND_SZ : REG_SZ;
            set_value(root, v->key, v->name, type, v->sz, (lstrlenW(v->sz) + 1) * sizeof(WCHAR));
        } else {
            set_value(root, v->key, v->name, REG_DWORD, &v->dw, sizeof(v->dw));
        }
    }
}

static void enable_privilege(const WCHAR *name)
{
    HANDLE token;
    TOKEN_PRIVILEGES tp = {.PrivilegeCount = 1};
    if (!OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES, &token)) return;
    if (LookupPrivilegeValueW(NULL, name, &tp.Privileges[0].Luid)) {
        tp.Privileges[0].Attributes = SE_PRIVILEGE_ENABLED;
        AdjustTokenPrivileges(token, FALSE, &tp, 0, NULL, NULL);
    }
    CloseHandle(token);
}

static void run(const WCHAR *command, const WCHAR *cwd)
{
    WCHAR line[MAX_PATH * 2];
    lstrcpynW(line, command, ARRAYSIZE(line));
    STARTUPINFOW si = {.cb = sizeof(si), .dwFlags = STARTF_USESHOWWINDOW, .wShowWindow = SW_HIDE};
    PROCESS_INFORMATION pi;
    if (!CreateProcessW(NULL, line, NULL, NULL, FALSE, CREATE_NO_WINDOW, NULL, cwd, &si, &pi)) return;
    WaitForSingleObject(pi.hProcess, INFINITE);
    CloseHandle(pi.hThread);
    CloseHandle(pi.hProcess);
}

static void copy_to_system(const WCHAR *name, const WCHAR *subdir, WCHAR *target)
{
    WCHAR source[MAX_PATH];
    path(source, name);
    GetSystemDirectoryW(target, MAX_PATH);
    wsprintfW(target + lstrlenW(target), L"%s\\%s", subdir, name);
    CopyFileW(source, target, FALSE);
}

static void update_driver(const WCHAR *hardware_id, const WCHAR *inf)
{
    HMODULE newdev = LoadLibraryW(L"newdev.dll");
    UPDATE_DRIVER update = newdev ? (UPDATE_DRIVER)GetProcAddress(newdev, "UpdateDriverForPlugAndPlayDevicesW") : NULL;
    BOOL reboot;
    if (update) update(NULL, hardware_id, inf, 1 /* INSTALLFLAG_FORCE */, &reboot);
}

/* Display: bind the Bochs VBE adapter to v86vid through its INF */
static void install_display(void)
{
    WCHAR inf[MAX_PATH];
    path(inf, L"v86vid.inf");
    update_driver(L"PCI\\VEN_1234&DEV_1111", inf);
}

/* Sound: the Sound Blaster 16 is not Plug and Play, so add it as a root device like Add Hardware does; the INF's
   first resource choices (220h, IRQ 5, DMA 1 and 5) are what v86 emulates */
static void install_sound(void)
{
    static const WCHAR hardware_id[] = L"*PNPB003\0";
    HDEVINFO set = SetupDiCreateDeviceInfoList(&media_class, NULL);
    if (set == INVALID_HANDLE_VALUE) return;
    SP_DEVINFO_DATA device = {.cbSize = sizeof(device)};
    if (SetupDiCreateDeviceInfoW(set, L"MEDIA", &media_class, NULL, NULL, DICD_GENERATE_ID, &device) &&
        SetupDiSetDeviceRegistryPropertyW(set, &device, SPDRP_HARDWAREID, (const BYTE *)hardware_id,
                                          sizeof(hardware_id)) &&
        SetupDiCallClassInstaller(DIF_REGISTERDEVICE, set, &device)) {
        WCHAR inf[MAX_PATH];
        ExpandEnvironmentStringsW(L"%SystemRoot%\\inf\\wdma_ctl.inf", inf, MAX_PATH);
        update_driver(L"*PNPB003", inf);
    }
    SetupDiDestroyDeviceInfoList(set);
}

/* Network: v86's NE2000 is a Realtek RTL8029, XP has the driver; the site connects it to the LAN relay */
static void install_network(void)
{
    WCHAR inf[MAX_PATH];
    ExpandEnvironmentStringsW(L"%SystemRoot%\\inf\\netrtpnt.inf", inf, MAX_PATH);
    update_driver(L"PCI\\VEN_10EC&DEV_8029", inf);
}

/* Mouse: system start filter above mouclass, also the host channel for the agent */
static void install_mouse(void)
{
    WCHAR target[MAX_PATH];
    copy_to_system(L"v86mou.sys", L"\\drivers", target);
    SC_HANDLE scm = OpenSCManagerW(NULL, NULL, SC_MANAGER_CREATE_SERVICE);
    if (scm) {
        SC_HANDLE service = CreateServiceW(scm, L"v86mou", L"v86 Mouse and Host Channel", SERVICE_ALL_ACCESS,
                                           SERVICE_KERNEL_DRIVER, SERVICE_SYSTEM_START, SERVICE_ERROR_NORMAL,
                                           L"system32\\drivers\\v86mou.sys", L"Pointer Class", NULL, NULL, NULL,
                                           NULL);
        if (service) CloseServiceHandle(service);
        CloseServiceHandle(scm);
    }
    static const WCHAR filters[] = L"mouclass\0v86mou\0";
    set_value(HKEY_LOCAL_MACHINE, CCS L"Control\\Class\\{4D36E96F-E325-11CE-BFC1-08002BE10318}", L"UpperFilters",
              REG_MULTI_SZ, filters, sizeof(filters));
}

/* Agent: start at every logon, and stand in as the web browser */
static void install_agent(void)
{
    WCHAR target[MAX_PATH];
    copy_to_system(L"v86agent.exe", L"", target);
    set_value(HKEY_LOCAL_MACHINE, CV L"Run", L"v86agent", REG_SZ, target, (lstrlenW(target) + 1) * sizeof(WCHAR));
    RegDeleteKeyW(HKEY_LOCAL_MACHINE, L"SOFTWARE\\Classes\\http\\shell\\open\\ddeexec\\Application");
    RegDeleteKeyW(HKEY_LOCAL_MACHINE, L"SOFTWARE\\Classes\\http\\shell\\open\\ddeexec\\Topic");
    RegDeleteKeyW(HKEY_LOCAL_MACHINE, L"SOFTWARE\\Classes\\http\\shell\\open\\ddeexec");
    RegDeleteKeyW(HKEY_LOCAL_MACHINE, L"SOFTWARE\\Classes\\https\\shell\\open\\ddeexec\\Application");
    RegDeleteKeyW(HKEY_LOCAL_MACHINE, L"SOFTWARE\\Classes\\https\\shell\\open\\ddeexec\\Topic");
    RegDeleteKeyW(HKEY_LOCAL_MACHINE, L"SOFTWARE\\Classes\\https\\shell\\open\\ddeexec");
}

static void tune_machine(void)
{
    set_values(HKEY_LOCAL_MACHINE, machine, ARRAYSIZE(machine));
    for (size_t i = 0; i < ARRAYSIZE(services); i++) {
        WCHAR key[MAX_PATH];
        wsprintfW(key, CCS L"Services\\%s", services[i]);
        DWORD disabled = SERVICE_DISABLED;
        HKEY h;
        if (RegOpenKeyExW(HKEY_LOCAL_MACHINE, key, 0, KEY_SET_VALUE, &h) != ERROR_SUCCESS) continue;
        RegSetValueExW(h, L"Start", 0, REG_DWORD, (const BYTE *)&disabled, sizeof(disabled));
        RegCloseKey(h);
    }
    /* Japanese text (like song names) falls back to MS Gothic in the interface fonts */
    static const WCHAR gothic[] = L"MSGOTHIC.TTC,MS UI Gothic\0";
    static const WCHAR *const linked[] = {L"Tahoma", L"MS Shell Dlg", L"Microsoft Sans Serif", L"Trebuchet MS"};
    for (size_t i = 0; i < ARRAYSIZE(linked); i++)
        set_value(HKEY_LOCAL_MACHINE, L"SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\FontLink\\SystemLink",
                  linked[i], REG_MULTI_SZ, gothic, sizeof(gothic));
    /* No pagefile and no hibernation file: the disk streams over the network */
    static const WCHAR no_paging[] = L"\0";
    set_value(HKEY_LOCAL_MACHINE, CCS L"Control\\Session Manager\\Memory Management", L"PagingFiles", REG_MULTI_SZ,
              no_paging, sizeof(no_paging));
    HKEY h;
    if (RegOpenKeyExW(HKEY_LOCAL_MACHINE, L"SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Winlogon", 0,
                      KEY_SET_VALUE, &h) == ERROR_SUCCESS) {
        RegDeleteValueW(h, L"AutoLogonCount");
        RegCloseKey(h);
    }
    run(L"powercfg.exe /hibernate off", NULL);
}

/* Every event of the sound scheme gets no sound */
static void silence_sounds(HKEY root)
{
    HKEY apps;
    if (RegOpenKeyExW(root, L"AppEvents\\Schemes\\Apps", 0, KEY_READ, &apps) != ERROR_SUCCESS) return;
    WCHAR app[MAX_PATH], event[MAX_PATH], key[MAX_PATH * 3];
    for (DWORD i = 0, n = MAX_PATH; RegEnumKeyExW(apps, i, app, &n, NULL, NULL, NULL, NULL) == ERROR_SUCCESS;
         i++, n = MAX_PATH) {
        HKEY events;
        if (RegOpenKeyExW(apps, app, 0, KEY_READ, &events) != ERROR_SUCCESS) continue;
        for (DWORD j = 0, m = MAX_PATH; RegEnumKeyExW(events, j, event, &m, NULL, NULL, NULL, NULL) == ERROR_SUCCESS;
             j++, m = MAX_PATH) {
            wsprintfW(key, L"AppEvents\\Schemes\\Apps\\%s\\%s\\.Current", app, event);
            set_value(root, key, NULL, REG_SZ, L"", sizeof(WCHAR));
        }
        RegCloseKey(events);
    }
    RegCloseKey(apps);
}

static void tune_user(HKEY root)
{
    set_values(root, user, ARRAYSIZE(user));
    set_value(root, DESKTOP, L"UserPreferencesMask", REG_BINARY, preferences_mask, sizeof(preferences_mask));
    silence_sounds(root);
}

/* Settings go into the Default User profile, so the new account starts with them, but the per user setup of Windows
   (Active Setup) resets some at the first logon, so the account applies them once more after it, see user_logon() */
static void tune_default_user(void)
{
    WCHAR hive[MAX_PATH];
    DWORD size = MAX_PATH;
    HKEY profiles;
    if (RegOpenKeyExW(HKEY_LOCAL_MACHINE, L"SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\ProfileList", 0,
                      KEY_QUERY_VALUE, &profiles) != ERROR_SUCCESS)
        return;
    WCHAR dir_raw[MAX_PATH];
    LONG status = RegQueryValueExW(profiles, L"DefaultUserProfile", NULL, NULL, (BYTE *)hive, &size);
    size = sizeof(dir_raw);
    if (status == ERROR_SUCCESS)
        status = RegQueryValueExW(profiles, L"ProfilesDirectory", NULL, NULL, (BYTE *)dir_raw, &size);
    RegCloseKey(profiles);
    if (status != ERROR_SUCCESS) return;
    WCHAR profiles_dir[MAX_PATH], file[MAX_PATH];
    ExpandEnvironmentStringsW(dir_raw, profiles_dir, MAX_PATH);
    wsprintfW(file, L"%s\\%s\\NTUSER.DAT", profiles_dir, hive);

    if (RegLoadKeyW(HKEY_USERS, DEFAULT_HIVE, file) != ERROR_SUCCESS) return;
    HKEY root;
    if (RegOpenKeyExW(HKEY_USERS, DEFAULT_HIVE, 0, KEY_ALL_ACCESS, &root) == ERROR_SUCCESS) {
        tune_user(root);
        WCHAR command[MAX_PATH];
        wsprintfW(command, L"\"%s\\v86setup.exe\" /user", dir);
        set_value(root, L"Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce", L"v86user", REG_SZ, command,
                  (lstrlenW(command) + 1) * sizeof(WCHAR));
        RegCloseKey(root);
    }
    RegUnLoadKeyW(HKEY_USERS, DEFAULT_HIVE);
}

/* The account the desktop logs on with, with its picture on the Start menu */
static void create_user(void)
{
    USER_INFO_1 info = {.usri1_name = USER_NAME,
                        .usri1_password = L"",
                        .usri1_priv = USER_PRIV_USER,
                        .usri1_flags = UF_SCRIPT | UF_DONT_EXPIRE_PASSWD};
    NetUserAdd(NULL, 1, (BYTE *)&info, NULL);
    USER_INFO_1011 full_name = {.usri1011_full_name = USER_FULL_NAME};
    NetUserSetInfo(NULL, USER_NAME, 1011, (BYTE *)&full_name, NULL);
    LOCALGROUP_MEMBERS_INFO_3 member = {.lgrmi3_domainandname = USER_NAME};
    NetLocalGroupAddMembers(NULL, L"Administrators", 3, (BYTE *)&member, 1);

    WCHAR source[MAX_PATH], target[MAX_PATH];
    path(source, L"user.bmp");
    ExpandEnvironmentStringsW(L"%ALLUSERSPROFILE%\\Application Data\\Microsoft\\User Account Pictures\\" USER_NAME
                              L".bmp",
                              target, MAX_PATH);
    CopyFileW(source, target, FALSE);
}

/* Offline activation: activate.vbs feeds the installation ID through confid.exe, it reports over COM1 */
static void activate(void)
{
    WCHAR command[MAX_PATH * 2];
    wsprintfW(command, L"cscript.exe //nologo \"%s\\activate.vbs\"", dir);
    run(command, dir);
}

/* First logon of the account, after Active Setup: the user settings again and the Media Player library, which resets
   some Media Player settings, so those once more after it */
static void user_logon(void)
{
    tune_user(HKEY_CURRENT_USER);
    WCHAR command[MAX_PATH * 2];
    wsprintfW(command, L"wscript.exe //B \"%s\\library.vbs\"", dir);
    run(command, dir);
    tune_user(HKEY_CURRENT_USER);
    SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, 0, (LPARAM)L"Environment", SMTO_ABORTIFHUNG, 5000, NULL);
}

int WINAPI wWinMain(HINSTANCE inst, HINSTANCE prev, LPWSTR cmd, int show)
{
    (void)inst;
    (void)prev;
    (void)show;
    GetModuleFileNameW(NULL, dir, MAX_PATH);
    *wcsrchr(dir, L'\\') = 0;
    if (!lstrcmpiW(cmd, L"/user")) {
        user_logon();
        return 0;
    }
    enable_privilege(SE_BACKUP_NAME);
    enable_privilege(SE_RESTORE_NAME);
    enable_privilege(SE_SHUTDOWN_NAME);

    install_mouse();
    install_agent();
    install_display();
    install_sound();
    install_network();
    tune_machine();
    tune_default_user();
    create_user();
    activate();
    ExitWindowsEx(EWX_REBOOT | EWX_FORCE, 0);
    return 0;
}
