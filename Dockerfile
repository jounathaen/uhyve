FROM lukemathwalker/cargo-chef:latest-rust-slim-buster as planner
RUN rustup default nightly
WORKDIR /uhyve
COPY . .
# Cargo chef cook uhyve
RUN cargo chef prepare --recipe-path recipe.json
# Cargo chef cook the teskernels
WORKDIR /uhyve/tests/test-kernels
RUN cargo chef prepare --recipe-path recipe.json


# Prebuild dependencies of uhyve
FROM lukemathwalker/cargo-chef:latest-rust-slim-buster as cacher
WORKDIR /uhyve
COPY --from=planner /uhyve/recipe.json recipe.json
COPY ./rust-toolchain /uhyve
RUN cargo chef cook --recipe-path recipe.json --tests


# Prebuild dependencies of the testkernels
FROM lukemathwalker/cargo-chef:latest-rust-slim-buster as no-std-cacher
RUN apt update && apt install -y libssl-dev pkg-config
RUN cargo install cargo-download
COPY ./tests/test-kernels /test-kernels
WORKDIR /test-kernels
COPY --from=planner /uhyve/tests/test-kernels/recipe.json recipe.json
RUN cargo chef cook --recipe-path recipe.json


FROM rust:slim-buster
ARG ci_project_dir
RUN echo $ci_project_dir
WORKDIR $ci_project_dir
#COPY . .
COPY ./tests/test-kernels /tests/test-kernels
# Copy over the cached dependencies
COPY --from=cacher /uhyve/target $ci_project_dir/target
COPY --from=no-std-cacher /test-kernels/target $ci_project_dir/tests/test-kernels/target
COPY --from=cacher $CARGO_HOME $CARGO_HOME
COPY --from=no-std-cacher $CARGO_HOME $CARGO_HOME
WORKDIR $ci_project_dir/tests/test-kernels
RUN cargo
WORKDIR $ci_project_dir
