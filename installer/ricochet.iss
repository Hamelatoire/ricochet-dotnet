; Builds the Windows installer for Ricochet with Inno Setup 6.
;
; It packages what `dotnet publish` produced, so publish first:
;   dotnet publish src/Ricochet.App -c Release -r win-x64 -o out/app
;   dotnet publish src/Ricochet.Cli -c Release -r win-x64 -o out/cli
;   ISCC /DAppVersion=0.2.0 /DVersionTag=v0.2.0 installer\ricochet.iss
;
; The installer is written to dist\. The release workflow does these steps, and so does the pull request check,
; so a change that breaks the installer is found before it merges.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef VersionTag
  #define VersionTag "v" + AppVersion
#endif

[Setup]
; A fixed id lets a newer installer find and replace an older install, and lets Windows list one uninstaller.
AppId={{B2F0C6E4-5E2D-4C8B-9A41-7C0E6F3D2A19}
AppName=Ricochet
AppVersion={#AppVersion}
AppPublisher=Ricochet
DefaultDirName={autopf}\Ricochet
DisableProgramGroupPage=yes
; Installs for the current user, in their own programs folder, with no administrator prompt.
; The wizard still offers an all-users install to someone who wants one.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
LicenseFile=..\LICENSE
OutputDir=..\dist
OutputBaseFilename=ricochet-setup-{#VersionTag}-win-x64
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayName=Ricochet
UninstallDisplayIcon={app}\Ricochet.exe
; The icon of the setup program itself, and of its window.
SetupIconFile=..\src\Ricochet.App\Assets\ricochet.ico
; Closes a running Ricochet while updating it, so its files can be replaced.
CloseApplications=yes
RestartApplications=no
; Tells Windows the PATH changed, when the command line task was chosen.
ChangesEnvironment=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked
Name: "addtopath"; Description: "Add the &ricochet command to my PATH, to run tests from a terminal or CI script"; GroupDescription: "Command line:"; Flags: unchecked

[Files]
Source: "..\out\app\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
; The command line tool goes in its own folder, because its file name differs from the app's only by case.
Source: "..\out\cli\*"; DestDir: "{app}\cli"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\Ricochet"; Filename: "{app}\Ricochet.exe"
Name: "{autodesktop}\Ricochet"; Filename: "{app}\Ricochet.exe"; Tasks: desktopicon

[Registry]
; Always the current user's PATH, so the installer never edits the system PATH.
Root: HKCU; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}\cli"; Tasks: addtopath; Check: NeedsAddPath(ExpandConstant('{app}\cli'))

[Run]
Filename: "{app}\Ricochet.exe"; Description: "Start Ricochet"; Flags: nowait postinstall skipifsilent

[Code]
// True when the folder isn't already on the current user's PATH, so installing twice adds it once.
function NeedsAddPath(Folder: string): Boolean;
var
  Current: string;
begin
  if not RegQueryStringValue(HKCU, 'Environment', 'Path', Current) then
  begin
    Result := True;
    Exit;
  end;
  Result := Pos(';' + Uppercase(Folder) + ';', ';' + Uppercase(Current) + ';') = 0;
end;

// Takes the folder back out of the current user's PATH when Ricochet is uninstalled.
procedure RemoveFromPath(Folder: string);
var
  Current: string;
  Where: Integer;
begin
  if not RegQueryStringValue(HKCU, 'Environment', 'Path', Current) then
    Exit;

  Current := ';' + Current + ';';
  Where := Pos(';' + Uppercase(Folder) + ';', Uppercase(Current));
  if Where = 0 then
    Exit;

  // Removes ";folder", and keeps the ";" after it as the separator for what follows.
  Delete(Current, Where, Length(Folder) + 1);
  if (Length(Current) > 0) and (Current[1] = ';') then
    Delete(Current, 1, 1);
  if (Length(Current) > 0) and (Current[Length(Current)] = ';') then
    Delete(Current, Length(Current), 1);

  RegWriteExpandStringValue(HKCU, 'Environment', 'Path', Current);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RemoveFromPath(ExpandConstant('{app}\cli'));
end;
