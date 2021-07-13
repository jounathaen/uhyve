FROM lukemathwalker/cargo-chef:latest-rust-slim-buster as planner
RUN rustup default nightly
WORKDIR /__w/uhyve/uhyve
COPY . .
# Cargo chef cook uhyve
RUN cargo chef prepare --recipe-path recipe.json
# Cargo chef cook the teskernels
WORKDIR /__w/uhyve/uhyve/tests/test-kernels
RUN cargo chef prepare --recipe-path recipe.json


# Prebuild dependencies of uhyve
FROM lukemathwalker/cargo-chef:latest-rust-slim-buster as cacher
WORKDIR /__w/uhyve/uhyve
COPY --from=planner /__w/uhyve/uhyve/recipe.json recipe.json
COPY ./rust-toolchain /__w/uhyve/uhyve
RUN cargo chef cook --recipe-path recipe.json --tests


# Prebuild dependencies of the testkernels
FROM lukemathwalker/cargo-chef:latest-rust-slim-buster as no-std-cacher
RUN apt update && apt install -y libssl-dev pkg-config
RUN cargo install cargo-download
WORKDIR /__w/uhyve/uhyve/tests/test-kernels
COPY ./tests/test-kernels .
COPY --from=planner /__w/uhyve/uhyve/tests/test-kernels/recipe.json recipe.json
RUN cargo chef cook --recipe-path recipe.json


FROM rust:slim-buster
#ARG ci_project_dir
#RUN echo $ci_project_dir
#WORKDIR $ci_project_dir
#COPY . .
# Copy over the cached dependencies
COPY --from=cacher /__w/uhyve/uhyve/target /target
COPY --from=no-std-cacher /__w/uhyve/uhyve/tests/test-kernels/target /test-kernels-target
COPY --from=cacher $CARGO_HOME $CARGO_HOME
COPY --from=no-std-cacher $CARGO_HOME $CARGO_HOME
RUN apt update && apt install -y git
#COPY ./tests/test-kernels /tests/test-kernels
#WORKDIR /__w/uhyve/uhyve/tests/test-kernels
#RUN cargo
#WORKDIR /__w/uhyve/uhyve
