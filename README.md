# SCP Scripts Anthology
A collection of the scripts used by the French branch of SCP Foundation to collect data on the SCP websites or other Wikidot wikis.

## How to use
After compilation, use as follows: `scp-scripts-anthology script_name [parameters]`.

Global parameters for every script:
* `--branch` (`-b`): specify the branch you want to use the script on. Currently available: `en`, `fr`, `int`.
* `--site` (`-s`): directly enter the URL of the Wikidot you want to use the script on. Useful for using them on sandboxes. Don’t forget to put a `/` at the end.
* `--verbose` (`-v`): print in the console Crom queries and their response.

## Scripts
### list-pages
This script is like a search engine: it uses Crom to gather a list of pages corresponding to specific criteria. If no parameter is given, gives the list of all pages on the Wikidot (can take some time!).

Parameters:
* `--author` (`-a`): search the pages attributed to one specific author.
* `--all-tags` (`-T`): pages must include all following tags. Put them between quotation marks and separate each tag by a space.
* `--one-of-tags` (`-t`): pages must include one of the following tags. Put them between quotation marks and separate each tag by a space.