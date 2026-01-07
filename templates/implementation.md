---
name: Implementation
description: Execute staged TDD implementation from design document
category: implementation
---

# Implementation Template

Guide for implementing a system in stages using test-driven development.

## 1. Pre-Implementation Review

- [ ] Review the design document thoroughly
- [ ] Review any research documents referenced
- [ ] Confirm understanding of requirements and constraints
- [ ] Identify any ambiguities and resolve with user
- [ ] Verify development environment is ready

## 2. Stage Planning

- [ ] Break implementation into logical stages
- [ ] Order stages by dependency (foundational first)
- [ ] Define clear completion criteria for each stage
- [ ] Estimate complexity and identify risks per stage
- [ ] Present stage plan to user for approval

## 3. Stage Execution

Repeat for each stage:

### 3.1 Stage Setup

- [ ] State the goal of this stage
- [ ] List the components or features to implement
- [ ] Identify dependencies on previous stages

### 3.2 Write Tests First

- [ ] Write test cases that define expected behavior
- [ ] Cover happy path scenarios
- [ ] Cover edge cases and error conditions
- [ ] Verify tests fail (nothing implemented yet)

### 3.3 Implementation

- [ ] Implement the minimum code to pass tests
- [ ] Follow patterns established in design document
- [ ] Maintain consistency with existing codebase
- [ ] Run tests frequently during implementation

### 3.4 Stage Validation

- [ ] All tests for this stage pass
- [ ] Code follows project conventions
- [ ] No regressions in previous stages
- [ ] Implementation matches design intent

### 3.5 Stage Closure

- [ ] Document any deviations from design
- [ ] Note any technical debt incurred
- [ ] Update design document if needed
- [ ] Mark stage as complete before proceeding

## 4. Integration Validation

- [ ] Run full test suite
- [ ] Verify all components work together
- [ ] Test end-to-end scenarios
- [ ] Performance check if applicable

## 5. Final Review

- [ ] Compare implementation to original requirements
- [ ] Document any gaps or future work needed
- [ ] Clean up any temporary code or comments
- [ ] Present completed implementation to user
