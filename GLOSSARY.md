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

**Environment**:
A named set of Variables for one deployment target, such as staging or production. At most one is active at a time.
_Avoid_: Profile, config

**Variable**:
A named value referenced as `{{name}}` in a Request. It belongs to a Collection, an Environment, or the Globals.
_Avoid_: Placeholder, parameter

**Globals**:
The Variables available to every Request, whichever Collection or Environment is in use.
_Avoid_: Shared variables

**Secret**:
A Variable or auth value whose contents are kept out of shareable files and held only on the user's own machine.
_Avoid_: Credential, sensitive variable

**Workspace**:
The shareable body of work: Collections, Environments and Globals. It excludes History and Secrets, which stay personal.
_Avoid_: Account, profile, project

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

**Run**:
Sending a selection of Requests in order and checking their Assertions, without changing the Workspace.
_Avoid_: Monitor, test suite

**Import**:
A one-time conversion of an exported Postman file into the Workspace.
_Avoid_: Sync

**Spec link**:
A Collection kept up to date from an OpenAPI description at a URL the user chooses, by an update the user starts. Unlike an Import, it can be updated again.
_Avoid_: Sync, subscription
