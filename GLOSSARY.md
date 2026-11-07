# Ricochet

A fast, native desktop tool for building and sending HTTP requests, which can take over work exported from Postman.

## Language

**Request**:
A saved definition of an HTTP call: method, URL, headers and body.
_Avoid_: Call, endpoint

**Collection**:
A named group of Requests and Folders, with its own Variables.
_Avoid_: Project, workspace

**Folder**:
A named grouping of Requests and other Folders inside a Collection.
_Avoid_: Group, directory

**Folder tag**:
A label on a Folder that every Request inside it inherits, so a Run can include or exclude those Requests by tag.
_Avoid_: Category, group

**Hidden item**:
A Folder or Request left out of the sidebar and of Runs without being deleted. It stays on disk and can be shown again with one action.
_Avoid_: Deleted, archived

**Tab**:
An open Request shown in the strip across the top of the request view. Its unsent edits stay with it until they are saved or discarded.
_Avoid_: Window, document

**Command palette**:
A search list, opened with one shortcut, that finds and runs any action or Request by name.
_Avoid_: Menu, quick open

**Shortcut**:
A key combination that runs an action without the mouse, such as sending the open Request. Shortcuts are personal and never enter the Workspace.
_Avoid_: Hotkey, keybinding

**Environment**:
A named set of Variables for one deployment target, such as staging or production. At most one is active at a time.
_Avoid_: Profile, config

**Variable**:
A named value referenced as `{{name}}` in a Request. It belongs to a Collection, an Environment, or the Globals.
_Avoid_: Placeholder, parameter

**Prompt variable**:
A Variable marked so that the app asks for its value when a Request using it is sent. The answer is used for that send only, and is never saved as an Environment value.
_Avoid_: Input, one-time variable

**Globals**:
The Variables available to every Request, whichever Collection or Environment is in use.
_Avoid_: Shared variables

**Secret**:
A Variable or auth value whose contents are kept out of shareable files and held only on the user's own machine.
_Avoid_: Credential, sensitive variable

**Secret reference**:
A Variable value that points to a Secret kept elsewhere, such as an environment variable or a command the user writes. Only the pointer is saved, never the Secret it resolves to.
_Avoid_: Secret value, vault key

**.env file**:
A file named `.env` in the Workspace folder, read for local runs. Its Variables are treated as Secrets and are never saved.
_Avoid_: Config file, dotenv

**Workspace**:
The shareable body of work: Collections, Environments and Globals. It excludes History and Secrets, which stay personal.
_Avoid_: Account, profile, project

**Personal settings**:
Preferences for this person on this machine, such as the request timeout or the theme. They are kept apart from the Workspace, so a teammate's files cannot change them.
_Avoid_: Config, options

**Cookie jar**:
The cookies the app keeps between Requests for one Workspace and Environment. They are held only in memory, and a CLI Run starts with none.
_Avoid_: Cookie store, session

**History**:
The record of Requests that have been sent, with their outcomes.
_Avoid_: Log

**Assertion**:
One expected condition on a response, such as "status equals 200". It is stored on its Request, and shown as a test in the app.
_Avoid_: Test script, check

**Capture**:
A rule on a Request that saves a value from its response into a Variable for later Requests. The value itself is kept only in memory.
_Avoid_: Extract, post-response script

**Example**:
A response saved on a Request as an expected reply. Its credentials are masked, and it is stored in the Request's file.
_Avoid_: Sample, mock response

**Mock server**:
A server on this machine that answers Requests from Examples or from an OpenAPI description, only while the user has it running.
_Avoid_: Stub, fake API

**Run**:
Sending a selection of Requests in order and checking their Assertions, without changing the Workspace.
_Avoid_: Monitor, test suite

**Bulk run**:
Sending one Request once for each id in a pasted list, one at a time or several at once. It can resume after the last id it sent.
_Avoid_: Batch, loop

**Data file run**:
A Run of a Collection once per row of a CSV or JSON data file, with each row's values available as Variables for that row.
_Avoid_: Data-driven test, iteration

**Import**:
A one-time conversion of an exported file (Postman, Insomnia or OpenAPI) into the Workspace. Environments and whole Workspaces can also be brought in from Export files.
_Avoid_: Sync

**Export**:
A file written from a Collection, an Environment or a whole Workspace for another tool or person. Secrets are never included.
_Avoid_: Backup

**Spec link**:
A Collection kept up to date from an OpenAPI description at a URL the user chooses, by an update the user starts. Unlike an Import, it can be updated again.
_Avoid_: Sync, subscription
