---
name: Issue Fix
description: Systematically diagnose and fix a reported bug
category: issue
---

# Issue Fix Template

Guide for systematically diagnosing, fixing, and validating a reported issue.

## 1. Issue Understanding

- [ ] Read the issue report carefully
- [ ] Identify the expected behavior
- [ ] Identify the actual (buggy) behavior
- [ ] Note the conditions under which the bug occurs
- [ ] Gather any error messages, logs, or stack traces
- [ ] Ask clarifying questions if the report is incomplete

## 2. Issue Reproduction

- [ ] Set up the environment to match the report
- [ ] Follow steps to reproduce the issue
- [ ] Confirm the bug is reproducible
- [ ] Document exact reproduction steps
- [ ] If not reproducible, gather more information from reporter

## 3. Root Cause Analysis

- [ ] Trace the code path involved in the bug
- [ ] Identify where behavior diverges from expected
- [ ] Determine the root cause (not just the symptom)
- [ ] Understand why the bug was introduced
- [ ] Check if the bug affects other areas

## 4. Test Case Creation

- [ ] Write a test case that reproduces the bug
- [ ] Test should fail with current code
- [ ] Test should define the correct expected behavior
- [ ] Consider edge cases related to the bug

## 5. Fix Implementation

- [ ] Implement the minimal fix for the root cause
- [ ] Avoid unnecessary refactoring or scope creep
- [ ] Follow existing code patterns and conventions
- [ ] Consider backward compatibility if applicable

## 6. Fix Validation

- [ ] New test case passes
- [ ] All existing tests still pass
- [ ] Manually verify the fix in the original scenario
- [ ] Check related functionality for regressions

## 7. Documentation

- [ ] Document the fix in commit message
- [ ] Update any affected documentation
- [ ] Note if the fix has broader implications
- [ ] Close or update the issue report
