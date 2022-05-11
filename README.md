### How to collect the data

Merge `account_changes` and `action_receipt_actions` by `receipt_id`.

We have the natural order in these 2 arrays.
1. If `receipt_id` is stored in both arrays -> merge them to one line in the resulting table.
2. If `receipt_id` from `action_receipt_actions` has no pair in `account_changes` -> collect all the possible info from `action_receipt_actions` and put the line in the resulting table.
3. If the line in `account_changes` has no `receipt_id`, we need to check whether it changed someone's balance. If the balance was changed -> collect all the possible info from `account_changes` and put the line in the resulting table.

While merging, we can meet the situation #2 and #3 at the same point of time.
We need to find the right order of storing such cases.
I feel these 2 situations never affect each other, so any order will work fine.
I decided to put `account_changes` data first (just to be consistent)
