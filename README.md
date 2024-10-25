# SCP Scripts Anthology
A collection of the scripts used by the French branch of SCP Foundation to collect data on the SCP websites or other Wikidot wikis.

## How to use
After compilation, use as follows: `scp-scripts-anthology script_name [parameters]`.

Global parameters for every script:
* `--branch` (`-b`): specify the branch you want to use the script on. Currently available: `en`, `fr`, `int`.
* `--site` (`-s`): directly enter the URL of the Wikidot you want to use the script on. Useful for using them on sandboxes. Don’t forget to put a `/` at the end.
* `--verbose` (`-v`): print in the console Crom queries and their response.
* `--output` (`-o`): write the result in the given file.
* `--output-format` (`-f`): set the format of the output file. Available options: `yaml` (default), `json`.

## Scripts
### list-pages
This script is like a search engine: it uses Crom to gather a list of pages corresponding to specific criteria. If no parameter is given, gives the list of all pages on the Wikidot (can take some time!).

Parameters:
* `--author` (`-a`): search the pages attributed to one specific author.
* `--all-tags` (`-T`): pages must include all following tags. Put them between quotation marks and separate each tag by a space.
* `--one-of-tags` (`-t`): pages must include one of the following tags. Put them between quotation marks and separate each tag by a space.
* `--info` (`-i`): define the information requested from Crom, separated by spaces or commas. Default: `"url wikidotInfo.title"`.
* `--output-filter`: filter the information written in the output. Only the information specified will be written, if it has been requested in `--info`.
* `--source-contains`: keep the pages that matches the given regex. Can be used multiple times; see `--source-contains-any` and `--source-contains-all`. Must be used with a `--info` asking for at least `wikidotInfo.source`.
* `--source-contains--any` and `--source-contains-all`: set the source content filter to any/all. Pages whose source matches any/all of the `--source-contains` regexes will be kept. Default: `--source-contains-all`.
* `--source-contains-ignore-case`: ignore the case for all `--source-contains`. Not activated by default.